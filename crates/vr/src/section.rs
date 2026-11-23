//! The section plane held in the hand: it cuts away what lies on the hand's side of it, so the
//! controller pushed into the model shows the inside at its tip. Held, it goes with the
//! controller; left in place, it stays where it was in the model and moves with it.

use solidcraft_xr::view::Pose;

use crate::marks::Vertex;
use crate::room::Transform;
use crate::vector::{add, cross, dot, finite, scale, sub, turn, unit};

/// Half the side of the square drawn where the plane is (metres).
pub const HALF_SIDE: f64 = 0.15;

/// Half the width of the square's edges (metres).
pub const EDGE_HALF_WIDTH: f64 = 0.0015;

/// The vertices [`outline`] makes: four edges, each a quad seen from both sides.
pub const OUTLINE_VERTICES: usize = 4 * 12;

/// A plane: a point on it and the direction it cuts toward (unit). What lies on the side the
/// normal points to is cut away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
}

impl Plane {
    /// The plane at the tip of the controller `aim` points with (its −Z), cutting toward the
    /// hand: across the pointing direction. `None` when the pose is not one.
    pub fn at_tip(aim: &Pose) -> Option<Plane> {
        let [_, _, back] = aim.axes()?;
        Some(Plane { origin: aim.position, normal: back })
    }

    /// The room plane in the model's units, `transform` placing the model. `None` when a number
    /// is not one.
    pub fn to_model(&self, transform: &Transform) -> Option<Plane> {
        let plane = Plane { origin: transform.to_model(self.origin), normal: unit(transform.direction_to_model(self.normal))? };
        (finite(plane.origin) && finite(plane.normal)).then_some(plane)
    }

    /// The model plane in the room, `transform` placing the model.
    pub fn to_room(&self, transform: &Transform) -> Option<Plane> {
        let plane = Plane { origin: transform.apply(self.origin), normal: unit(turn(&transform.axes, self.normal))? };
        (finite(plane.origin) && finite(plane.normal)).then_some(plane)
    }

    /// The renderer's clip: the normal and its dot with the origin, as `f32` (fragments where
    /// `normal · p > d` are cut away). Worked out in `f64` and rounded once, in the model's own
    /// units about the model's origin, as the window's section is: the offset lands within an
    /// `f32` step of its size (about 1e-7 of it: within a millimetre at 1e7 mm from the origin),
    /// as fine as the model's own vertices, which the renderer also holds as `f32`. Placing the
    /// plane relative to the model's middle would need the renderer's clip to be so too.
    pub fn clip(&self) -> [f32; 4] {
        let [x, y, z] = self.normal.map(|v| v as f32);
        [x, y, z, dot(self.normal, self.origin) as f32]
    }

    /// Whether model point `p` is kept (on the side away from the normal, within a hair).
    pub fn keeps(&self, p: [f64; 3]) -> bool {
        dot(self.normal, sub(p, self.origin)) <= 1e-9
    }
}

/// A square of [`HALF_SIDE`] where room plane `plane` is, its edges in `colour` (linear light),
/// seen from both sides. Upright where the plane allows.
pub fn outline(plane: &Plane, colour: [f32; 3]) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(OUTLINE_VERTICES);
    let Some(n) = unit(plane.normal) else { return out };
    let Some(right) = unit(cross([0.0, 1.0, 0.0], n)).or_else(|| unit(cross(n, [0.0, 0.0, 1.0]))) else { return out };
    let up = cross(n, right);
    let at = |x: f64, y: f64| add(add(plane.origin, scale(right, x)), scale(up, y));
    let (h, w) = (HALF_SIDE, EDGE_HALF_WIDTH);
    // Each edge from one corner to the next, as a strip `w` wide inward and outward.
    let corners = [(-h, -h), (h, -h), (h, h), (-h, h)];
    for k in 0..4 {
        let (ax, ay) = corners[k];
        let (bx, by) = corners[(k + 1) % 4];
        // Outward across the edge: the edge's direction turned a quarter in the plane.
        let (dx, dy) = ((bx - ax) / (2.0 * h), (by - ay) / (2.0 * h));
        let (ox, oy) = (dy * w, -dx * w);
        let quad = [at(ax - ox, ay - oy), at(bx - ox, by - oy), at(bx + ox, by + oy), at(ax + ox, ay + oy)];
        let colour = [colour[0], colour[1], colour[2], 1.0];
        for i in [0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2] {
            out.push(Vertex { position: quad[i].map(|v| v as f32), colour });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::room::turn_about_vertical;

    /// Whether two points are the same but for rounding.
    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9)
    }

    /// A controller at `position` pointing down the room's −Z.
    fn ahead(position: [f64; 3]) -> Pose {
        Pose { position, orientation: [0.0, 0.0, 0.0, 1.0] }
    }

    /// The plane at the controller's tip faces back toward the hand: what lies past the tip is
    /// kept, what lies between it and the hand is cut away.
    #[test]
    fn the_held_plane_cuts_toward_the_hand() {
        let plane = Plane::at_tip(&ahead([0.1, 1.0, -0.5])).unwrap();
        assert_eq!(plane.origin, [0.1, 1.0, -0.5]);
        assert!(close(plane.normal, [0.0, 0.0, 1.0]));
        assert!(plane.keeps([0.1, 1.0, -0.9]), "past the tip");
        assert!(plane.keeps([5.0, -3.0, -0.5]), "on the plane");
        assert!(!plane.keeps([0.1, 1.0, -0.4]), "toward the hand");
        assert_eq!(plane.clip(), [0.0, 0.0, 1.0, -0.5]);
        // The controller turned a quarter to the left (pointing down −X): the plane turns with it.
        let (s, c) = std::f64::consts::FRAC_PI_4.sin_cos();
        let left = Plane::at_tip(&Pose { position: [0.0; 3], orientation: [0.0, s, 0.0, c] }).unwrap();
        assert!(close(left.normal, [1.0, 0.0, 0.0]), "{:?}", left.normal);
        assert!(left.keeps([-1.0, 0.0, 0.0]) && !left.keeps([1.0, 0.0, 0.0]));
    }

    /// The plane in the model's units and back: the same plane, and a point is kept in the
    /// room exactly when its model point is kept in the model.
    #[test]
    fn the_plane_goes_with_the_model() {
        // At real size, the model's origin a metre up: room (0.1, 1.2, 0.3) is model (100, 200, 300).
        let placed = Transform { translation: [0.0, 1.0, 0.0], ..Transform::MILLIMETRES };
        let room = Plane { origin: [0.1, 1.2, 0.3], normal: [0.0, 0.0, 1.0] };
        let model = room.to_model(&placed).unwrap();
        assert!(close(model.origin, [100.0, 200.0, 300.0]), "{:?}", model.origin);
        assert!(close(model.normal, [0.0, 0.0, 1.0]));
        // Turned a quarter about the vertical: the model's Z points along the room's X.
        let turned = Transform { axes: turn_about_vertical(std::f64::consts::FRAC_PI_2), ..placed };
        let sideways = Plane { origin: [0.0, 1.0, 0.0], normal: [1.0, 0.0, 0.0] }.to_model(&turned).unwrap();
        assert!(close(sideways.normal, [0.0, 0.0, 1.0]), "{:?}", sideways.normal);
        assert!(close(sideways.origin, [0.0; 3]));

        let t = Transform { axes: turn_about_vertical(0.7), scale: 0.002, translation: [1.0, -2.0, 0.5], pivot: [0.0; 3] };
        let plane = Plane { origin: [0.9, -1.8, 0.4], normal: unit([1.0, 2.0, -0.5]).unwrap() };
        let there = plane.to_model(&t).unwrap();
        let back = there.to_room(&t).unwrap();
        assert!(close(back.origin, plane.origin) && close(back.normal, plane.normal), "{back:?}");
        for p in [[0.0; 3], [100.0, -40.0, 7.0], [-50.0, 80.0, 300.0], [-120.0, -60.0, -20.0]] {
            assert_eq!(there.keeps(p), plane.keeps(t.apply(p)), "{p:?}");
        }
    }

    /// A plane far from the model's origin (1e7 mm) clips within an `f32` step there (one
    /// millimetre); one near the origin exactly where it is.
    #[test]
    fn a_far_plane_clips_within_an_f32_step_of_where_it_is() {
        let far = Plane { origin: [1.0e7 + 0.25, 3.0, -2.0], normal: [1.0, 0.0, 0.0] };
        let [x, y, z, d] = far.clip();
        assert_eq!([x, y, z], [1.0, 0.0, 0.0]);
        assert!((f64::from(d) - (1.0e7 + 0.25)).abs() <= 1.0, "{d}");
        let near = Plane { origin: [12.25, 3.0, -2.0], normal: [1.0, 0.0, 0.0] };
        assert_eq!(near.clip()[3], 12.25);
    }

    /// A pose that is no pose has no plane; a plane with no direction, or a model placed at no
    /// size or no place, has none in the other's units.
    #[test]
    fn a_plane_from_numbers_that_are_not_any_is_refused() {
        assert_eq!(Plane::at_tip(&Pose { position: [0.0; 3], orientation: [0.0; 4] }), None);
        assert_eq!(Plane::at_tip(&ahead([f64::NAN, 0.0, 0.0])), None);
        let flat = Plane { origin: [0.0; 3], normal: [0.0; 3] };
        assert_eq!(flat.to_model(&Transform::MILLIMETRES), None);
        assert_eq!(flat.to_room(&Transform::MILLIMETRES), None);
        let plane = Plane { origin: [0.0, 1.0, 0.0], normal: [0.0, 0.0, 1.0] };
        assert_eq!(plane.to_model(&Transform { scale: 0.0, ..Transform::MILLIMETRES }), None, "no size");
        assert_eq!(plane.to_model(&Transform { translation: [f64::NAN; 3], ..Transform::MILLIMETRES }), None);
        assert_eq!(plane.to_room(&Transform { translation: [f64::INFINITY, 0.0, 0.0], ..Transform::MILLIMETRES }), None);
        assert_eq!(Plane { normal: [f64::NAN; 3], ..plane }.to_model(&Transform::MILLIMETRES), None);
    }

    /// The outline is a square [`HALF_SIDE`] from the middle each way, its edges
    /// [`EDGE_HALF_WIDTH`] either side, all in the plane, upright, and seen from both sides.
    #[test]
    fn the_outline_is_an_upright_square_in_the_plane() {
        let plane = Plane { origin: [0.0, 1.0, -0.5], normal: [0.0, 0.0, 1.0] };
        let out = outline(&plane, [0.2, 0.4, 0.6]);
        assert_eq!(out.len(), OUTLINE_VERTICES);
        let reach = (HALF_SIDE + EDGE_HALF_WIDTH) as f32;
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for v in &out {
            assert!((v.position[2] + 0.5).abs() < 1e-6, "in the plane: {:?}", v.position);
            assert_eq!(v.colour, [0.2, 0.4, 0.6, 1.0]);
            for k in 0..2 {
                lo[k] = lo[k].min(v.position[k]);
                hi[k] = hi[k].max(v.position[k]);
            }
        }
        // Right is the room's X and up its Y: the square stands upright.
        assert!((lo[0] + reach).abs() < 1e-6 && (hi[0] - reach).abs() < 1e-6, "{lo:?} {hi:?}");
        assert!((lo[1] - (1.0 - reach)).abs() < 1e-6 && (hi[1] - (1.0 + reach)).abs() < 1e-6, "{lo:?} {hi:?}");
        // Seen from both sides: half the triangles face along the normal, half against it.
        let facing: Vec<f64> = out
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|i| t[i].position.map(f64::from));
                dot(cross(sub(b, a), sub(c, a)), plane.normal)
            })
            .collect();
        assert_eq!(facing.iter().filter(|f| **f > 0.0).count(), 8);
        assert_eq!(facing.iter().filter(|f| **f < 0.0).count(), 8);
    }

    /// A plane facing straight up has no upright: its square lies flat, still in the plane; a
    /// plane with no direction, or none that is a number, has no outline.
    #[test]
    fn a_flat_plane_still_has_an_outline_and_no_plane_has_none() {
        let floor = Plane { origin: [0.0, 0.8, 0.0], normal: [0.0, 1.0, 0.0] };
        let out = outline(&floor, [1.0; 3]);
        assert_eq!(out.len(), OUTLINE_VERTICES);
        assert!(out.iter().all(|v| (v.position[1] - 0.8).abs() < 1e-6));
        assert!(outline(&Plane { normal: [0.0; 3], ..floor }, [1.0; 3]).is_empty());
        assert!(outline(&Plane { normal: [f64::NAN, 1.0, 0.0], ..floor }, [1.0; 3]).is_empty());
        // A normal not of length 1 is made so: the square is the same size.
        let long = outline(&Plane { normal: [0.0, 5.0, 0.0], ..floor }, [1.0; 3]);
        assert_eq!(long, out);
    }
}
