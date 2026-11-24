//! The turns and frames Arrange lays bodies out with: a sheet's frame from its plane, a point in
//! that frame, the turn that lays a flat side down and squares it up, and the axis and angle of
//! a turn, which the Move feature that places a body takes.
//!
//! A turn is given by its columns: where X, Y and Z go. Moved from SolidCraft's Arrange command,
//! which worked these out itself.

use crate::math::Vec3;

/// No turn.
pub const IDENTITY: [Vec3; 3] = [
    Vec3::new(1.0, 0.0, 0.0),
    Vec3::new(0.0, 1.0, 0.0),
    Vec3::new(0.0, 0.0, 1.0),
];

/// Below this length a direction has none.
const SHORT: f64 = 1e-9;

/// `v` scaled to length 1, if it has a length past [`SHORT`].
fn unit(v: Vec3) -> Option<Vec3> {
    let l = v.length();
    (l > SHORT && l.is_finite()).then(|| v / l)
}

/// The frame of the plane along `x` with `y` in it: along `x`, across it in the plane and out
/// of the plane (right-handed, unit). `None` when `x` and `y` don't span a plane.
#[must_use]
pub fn plane_frame(x: Vec3, y: Vec3) -> Option<[Vec3; 3]> {
    let e1 = unit(x)?;
    let n = unit(e1.cross(unit(y)?))?;
    Some([e1, n.cross(e1), n])
}

/// `v` in the frame `rows`: how far it goes along each.
#[must_use]
pub fn in_frame(rows: [Vec3; 3], v: Vec3) -> Vec3 {
    let [a, b, c] = rows;
    Vec3::new(a.dot(v), b.dot(v), c.dot(v))
}

/// Whether the directions `a` and `b` (unit) are parallel or opposite, to within `tol`: the
/// sine of the angle between them.
#[must_use]
pub fn parallel(a: Vec3, b: Vec3, tol: f64) -> bool {
    a.cross(b).length() <= tol
}

/// `v` turned by `angle` (radians) about `axis` through the origin; `v` when `axis` has no
/// direction.
#[must_use]
pub fn turned_about(axis: Vec3, angle: f64, v: Vec3) -> Vec3 {
    let Some(k) = unit(axis) else { return v };
    let (sin, cos) = angle.sin_cos();
    v * cos + k.cross(v) * sin + k * (k.dot(v) * (1.0 - cos))
}

/// The shortest turn that points `n` (unit) along -Z; half a turn about X when `n` points up.
// An if-chain, not `f64::clamp`: the panic-site check reads named bounds as unproven.
#[allow(clippy::manual_clamp)]
#[must_use]
pub fn pointing_down(n: Vec3) -> [Vec3; 3] {
    let down = Vec3::new(0.0, 0.0, -1.0);
    let d = n.dot(down);
    let cos = if d > 1.0 {
        1.0
    } else if d < -1.0 {
        -1.0
    } else {
        d
    };
    if cos > 1.0 - 1e-15 {
        return IDENTITY;
    }
    let axis = unit(n.cross(down)).unwrap_or(Vec3::new(1.0, 0.0, 0.0));
    let angle = cos.acos();
    // Rodrigues' formula.
    let turn = |v: Vec3| {
        v * angle.cos() + axis.cross(v) * angle.sin() + axis * (axis.dot(v) * (1.0 - angle.cos()))
    };
    IDENTITY.map(turn)
}

/// `rotation` (one that lays a side down, [`pointing_down`]) followed by the turn about Z that
/// takes `edge`, a direction in that side before the turn, along X: quarter turns then line it
/// up with a sheet's edges. Unchanged when `edge` turned has no direction in XY.
#[must_use]
pub fn squared_up(rotation: [Vec3; 3], edge: Vec3) -> [Vec3; 3] {
    let [r0, r1, r2] = rotation;
    let e = r0 * edge.x + r1 * edge.y + r2 * edge.z;
    if e.x.hypot(e.y) <= 1e-9 {
        return rotation;
    }
    let a = -e.y.atan2(e.x);
    let (c, sn) = (a.cos(), a.sin());
    rotation.map(|v| Vec3::new(c * v.x - sn * v.y, sn * v.x + c * v.y, v.z))
}

/// Coordinate `i` of `v`, 0 past Z.
fn coord(v: Vec3, i: usize) -> f64 {
    v.to_array().get(i).copied().unwrap_or(0.0)
}

/// The axis (unit) and angle (radians, 0 to π) of the turn `r`.
#[must_use]
pub fn axis_angle(r: [Vec3; 3]) -> (Vec3, f64) {
    let [r0, r1, r2] = r;
    let trace = r0.x + r1.y + r2.z;
    // Entry (i, j) is r[j][i]; the skew part is 2 sin(angle) times the axis.
    let skew = Vec3::new(r1.z - r2.y, r2.x - r0.z, r0.y - r1.x);
    let (sin, cos) = (skew.length() / 2.0, (trace - 1.0) / 2.0);
    let angle = sin.atan2(cos);
    if angle < 1e-12 {
        return (Vec3::new(0.0, 0.0, 1.0), 0.0);
    }
    if cos > -0.5 {
        if let Some(axis) = unit(skew) {
            return (axis, angle);
        }
    }
    // Far round: the symmetric part less cos times I is (1 - cos) times the axis times itself;
    // its largest column, signed by the skew part where it says anything.
    let column = |j: usize| r.get(j).copied().unwrap_or(Vec3::new(0.0, 0.0, 0.0));
    let sym = |j: usize| {
        let e = |i: usize| {
            let (aij, aji) = (coord(column(j), i), coord(column(i), j));
            (aij + aji) / 2.0 - if i == j { cos } else { 0.0 }
        };
        Vec3::new(e(0), e(1), e(2))
    };
    let k = (0..3)
        .max_by(|a, b| sym(*a).length().total_cmp(&sym(*b).length()))
        .unwrap_or(0);
    let mut axis = unit(sym(k)).unwrap_or(Vec3::new(0.0, 0.0, 1.0));
    if axis.dot(skew) < 0.0 {
        axis *= -1.0;
    }
    (axis, angle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-12
    }

    #[test]
    fn a_plane_frame_is_square_and_right_handed() {
        let Some([e1, e2, n]) = plane_frame(Vec3::new(2.0, 0.0, 0.0), Vec3::new(1.0, 3.0, 0.0))
        else {
            panic!("a plane")
        };
        assert!(near(e1, Vec3::new(1.0, 0.0, 0.0)));
        assert!(near(e2, Vec3::new(0.0, 1.0, 0.0)));
        assert!(near(n, Vec3::new(0.0, 0.0, 1.0)));
        assert!(plane_frame(Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn a_side_pointed_down_points_down() {
        for n in [
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, -0.6, 0.8),
            Vec3::new(0.6, 0.0, 0.8),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 0.0, -1.0),
        ] {
            let r = pointing_down(n);
            let [a, b, c] = r;
            assert!(
                near(a * n.x + b * n.y + c * n.z, Vec3::new(0.0, 0.0, -1.0)),
                "{n:?}"
            );
        }
    }

    #[test]
    fn a_squared_up_edge_runs_along_x() {
        let r = squared_up(IDENTITY, Vec3::new(1.0, 1.0, 0.0));
        let [a, b, c] = r;
        let e = a + b + c * 0.0;
        assert!(e.y.abs() < 1e-12 && e.x > 0.0);
    }

    #[test]
    fn turns_compose_into_one_axis_and_angle() {
        let turn = |axis: Vec3, a: f64| IDENTITY.map(|v| turned_about(axis, a, v));
        for (axis, a) in [
            (Vec3::new(1.0, 0.0, 0.0), 0.3),
            (Vec3::new(1.0, 2.0, -0.5), 2.0),
            (Vec3::new(0.0, 1.0, 0.0), std::f64::consts::PI),
            (Vec3::new(1.0, 1.0, 1.0), std::f64::consts::PI - 1e-9),
            (Vec3::new(-0.2, 0.1, 1.0), 1e-13),
        ] {
            let r = turn(axis, a);
            let (got_axis, got) = axis_angle(r);
            let back = turn(got_axis, got);
            for (b, w) in back.iter().zip(r) {
                assert!((*b - w).length() < 1e-7, "{axis:?} {a}: {got_axis:?} {got}");
            }
        }
        assert_eq!(axis_angle(IDENTITY), (Vec3::new(0.0, 0.0, 1.0), 0.0));
    }

    #[test]
    fn in_frame_reads_along_each_row() {
        let rows = [
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ];
        assert!(near(
            in_frame(rows, Vec3::new(1.0, 2.0, 3.0)),
            Vec3::new(2.0, -1.0, 3.0)
        ));
        assert!(parallel(rows[2], Vec3::new(0.0, 0.0, -1.0), 1e-9));
        assert!(!parallel(rows[0], rows[2], 1e-9));
    }
}
