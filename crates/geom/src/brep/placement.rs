//! A record moved by a rigid motion: a rotation and a translation. Every point, axis and control
//! point moves; every parameter, pcurve, tolerance, orientation and the canonical order stay as
//! they are, because a rigid motion keeps distances, angles and handedness. The moved record is
//! the same B-rep in other coordinates, exactly. Reflections and scalings change parameters or
//! geometry kinds; they are a kernel operation, not this.
use super::{Brep, Curve, Frame, Surface};

/// A rotation (rows of an orthonormal matrix with determinant +1) and a translation:
/// `p ↦ rotation · p + translation`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rigid {
    pub rotation: [[f64; 3]; 3],
    pub translation: [f64; 3],
}

/// How far the rotation may be from orthonormal (per entry of RᵀR − I) and its determinant from 1.
const ORTHONORMAL: f64 = 1e-9;

impl Rigid {
    /// The rigid motion of a column-major affine matrix (columns 0–2 the images of x, y and z,
    /// column 3 the translation), or `None` when its linear part scales, shears or reflects.
    pub fn of_matrix(m: &[[f64; 4]; 4]) -> Option<Rigid> {
        if m.iter().flatten().any(|x| !x.is_finite()) || m[0][3] != 0.0 || m[1][3] != 0.0 || m[2][3] != 0.0 || m[3][3] != 1.0 {
            return None;
        }
        let rotation = [[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]];
        let column = |j: usize| [rotation[0][j], rotation[1][j], rotation[2][j]];
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        for i in 0..3 {
            for j in 0..3 {
                let want = if i == j { 1.0 } else { 0.0 };
                if (dot(column(i), column(j)) - want).abs() > ORTHONORMAL {
                    return None;
                }
            }
        }
        let [a, b, c] = rotation;
        let det = a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]);
        ((det - 1.0).abs() <= ORTHONORMAL).then_some(Rigid { rotation, translation: [m[3][0], m[3][1], m[3][2]] })
    }

    fn direction(&self, d: [f64; 3]) -> [f64; 3] {
        self.rotation.map(|row| row[0] * d[0] + row[1] * d[1] + row[2] * d[2])
    }

    fn point(&self, p: [f64; 3]) -> [f64; 3] {
        let r = self.direction(p);
        [r[0] + self.translation[0], r[1] + self.translation[1], r[2] + self.translation[2]]
    }

    /// A homogeneous control point `[x w, y w, z w, w]`: the point moves, its weight stays.
    fn homogeneous(&self, h: [f64; 4]) -> [f64; 4] {
        let r = self.direction([h[0], h[1], h[2]]);
        let w = h[3];
        [r[0] + w * self.translation[0], r[1] + w * self.translation[1], r[2] + w * self.translation[2], w]
    }

    /// The column-major affine matrix of this motion.
    fn matrix(&self) -> [[f64; 4]; 4] {
        let (r, t) = (self.rotation, self.translation);
        [[r[0][0], r[1][0], r[2][0], 0.0], [r[0][1], r[1][1], r[2][1], 0.0], [r[0][2], r[1][2], r[2][2], 0.0], [t[0], t[1], t[2], 1.0]]
    }

    fn frame(&self, f: &Frame) -> Frame {
        Frame { origin: self.point(f.origin), x_axis: self.direction(f.x_axis), y_axis: self.direction(f.y_axis) }
    }

    fn curve(&self, c: &Curve) -> Curve {
        match c {
            Curve::Line { start, end } => Curve::Line { start: self.point(*start), end: self.point(*end) },
            Curve::Circle { frame, radius } => Curve::Circle { frame: self.frame(frame), radius: *radius },
            Curve::Ellipse { frame, radii } => Curve::Ellipse { frame: self.frame(frame), radii: *radii },
            Curve::Polyline { points } => Curve::Polyline { points: points.iter().map(|p| self.point(*p)).collect() },
            Curve::Spline { knots, points } => Curve::Spline { knots: knots.clone(), points: points.iter().map(|p| self.point(*p)).collect() },
            Curve::RationalSpline { knots, homogeneous } => {
                Curve::RationalSpline { knots: knots.clone(), homogeneous: homogeneous.iter().map(|h| self.homogeneous(*h)).collect() }
            }
            Curve::SurfaceIntersection { surfaces, leader, tolerance } => Curve::SurfaceIntersection {
                surfaces: [Box::new(self.surface(&surfaces[0])), Box::new(self.surface(&surfaces[1]))],
                leader: Box::new(self.curve(leader)),
                tolerance: *tolerance,
            },
        }
    }

    fn surface(&self, s: &Surface) -> Surface {
        match s {
            Surface::Plane { origin, u_axis, v_axis } => {
                Surface::Plane { origin: self.point(*origin), u_axis: self.direction(*u_axis), v_axis: self.direction(*v_axis) }
            }
            Surface::PlaneThrough { origin, u_point, v_point } => {
                Surface::PlaneThrough { origin: self.point(*origin), u_point: self.point(*u_point), v_point: self.point(*v_point) }
            }
            Surface::Cylinder { frame, radius } => Surface::Cylinder { frame: self.frame(frame), radius: *radius },
            Surface::Cone { frame, radius_at_origin, half_angle } => {
                Surface::Cone { frame: self.frame(frame), radius_at_origin: *radius_at_origin, half_angle: *half_angle }
            }
            Surface::Sphere { frame, radius } => Surface::Sphere { frame: self.frame(frame), radius: *radius },
            Surface::Torus { frame, major_radius, minor_radius } => {
                Surface::Torus { frame: self.frame(frame), major_radius: *major_radius, minor_radius: *minor_radius }
            }
            Surface::Spline { u_knots, v_knots, points } => Surface::Spline {
                u_knots: u_knots.clone(),
                v_knots: v_knots.clone(),
                points: points.iter().map(|row| row.iter().map(|p| self.point(*p)).collect()).collect(),
            },
            Surface::RationalSpline { u_knots, v_knots, homogeneous } => Surface::RationalSpline {
                u_knots: u_knots.clone(),
                v_knots: v_knots.clone(),
                homogeneous: homogeneous.iter().map(|row| row.iter().map(|h| self.homogeneous(*h)).collect()).collect(),
            },
            // The revolution's own placement is composed with the motion; profile and axis stay
            // where the producer put them.
            Surface::PlacedRevolution { profile, origin, axis, matrix, forward } => Surface::PlacedRevolution {
                profile: profile.clone(),
                origin: *origin,
                axis: *axis,
                matrix: compose(&self.matrix(), matrix),
                forward: *forward,
            },
        }
    }
}

/// `a · b` of column-major 4×4 matrices.
fn compose(a: &[[f64; 4]; 4], b: &[[f64; 4]; 4]) -> [[f64; 4]; 4] {
    let mut out = [[0.0; 4]; 4];
    for (col, out_col) in out.iter_mut().enumerate() {
        for (row, cell) in out_col.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[k][row] * b[col][k]).sum();
        }
    }
    out
}

impl Brep {
    /// This record moved by `motion` (module docs): topology, parameters and pcurves unchanged.
    pub fn moved(&self, motion: &Rigid) -> Brep {
        let mut out = self.clone();
        for f in &mut out.faces {
            f.surface = motion.surface(&f.surface);
            f.nurbs = f.nurbs.as_ref().map(|n| motion.surface(n));
        }
        for e in &mut out.edges {
            e.curve = motion.curve(&e.curve);
            e.nurbs = e.nurbs.as_ref().map(|n| motion.curve(n));
        }
        for v in &mut out.vertices {
            v.point = motion.point(v.point);
        }
        out
    }
}
