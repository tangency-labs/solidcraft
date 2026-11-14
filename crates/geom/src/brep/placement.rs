//! A record moved by a rigid motion: a rotation and a translation. Every point, axis and control
//! point moves; every parameter, pcurve, tolerance, orientation and the canonical order stay as
//! they are, because a rigid motion keeps distances, angles and handedness. The moved record is
//! the same B-rep in other coordinates, exactly. A scaling or shear ([`mapped`]) keeps a record
//! of planes, splines, lines and conics the same way (a circle becomes an ellipse), its
//! tolerances grown; it changes the kinds or
//! parameters of the other geometry, and a reflection the orientation: those are a kernel
//! operation, not this.
use solidcore_record::brep::{Brep, Curve, Frame, Surface};

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

    fn frame(&self, f: &Frame) -> Option<Frame> {
        let [x_axis, y_axis] = solidcore_ops::affine::orthonormal_axes(self.direction(f.x_axis), self.direction(f.y_axis)).ok()?;
        Some(Frame { origin: self.point(f.origin), x_axis, y_axis })
    }

    /// The curve moved; `None` for one on a blend by its definition ([`moved`]).
    fn curve(&self, c: &Curve) -> Option<Curve> {
        Some(match c {
            Curve::Line { start, end } => Curve::Line { start: self.point(*start), end: self.point(*end) },
            Curve::Circle { frame, radius } => Curve::Circle { frame: self.frame(frame)?, radius: *radius },
            Curve::Ellipse { frame, radii } => Curve::Ellipse { frame: self.frame(frame)?, radii: *radii },
            Curve::Polyline { points } => Curve::Polyline { points: points.iter().map(|p| self.point(*p)).collect() },
            Curve::Spline { knots, points } => Curve::Spline { knots: knots.clone(), points: points.iter().map(|p| self.point(*p)).collect() },
            Curve::RationalSpline { knots, homogeneous } => {
                Curve::RationalSpline { knots: knots.clone(), homogeneous: homogeneous.iter().map(|h| self.homogeneous(*h)).collect() }
            }
            Curve::SurfaceIntersection { surfaces, leader, tolerance } => Curve::SurfaceIntersection {
                surfaces: [Box::new(self.surface(&surfaces[0])?), Box::new(self.surface(&surfaces[1])?)],
                leader: Box::new(self.curve(leader)?),
                tolerance: *tolerance,
            },
        })
    }

    /// The surface moved; `None` for a blend by its definition, which the kernel moves with its
    /// net ([`moved`]).
    fn surface(&self, s: &Surface) -> Option<Surface> {
        Some(match s {
            Surface::Plane { origin, u_axis, v_axis } => {
                let [u_axis, v_axis] = solidcore_ops::affine::rigid_plane_axes(&self.matrix(), *u_axis, *v_axis).ok()?;
                Surface::Plane { origin: self.point(*origin), u_axis, v_axis }
            }
            Surface::PlaneThrough { origin, u_point, v_point } => {
                let (origin, [u_axis, v_axis]) = solidcore_ops::affine::rigid_plane_through(&self.matrix(), *origin, *u_point, *v_point).ok()?;
                Surface::Plane { origin, u_axis, v_axis }
            }
            Surface::Cylinder { frame, radius } => Surface::Cylinder { frame: self.frame(frame)?, radius: *radius },
            Surface::Cone { frame, radius_at_origin, half_angle } => {
                Surface::Cone { frame: self.frame(frame)?, radius_at_origin: *radius_at_origin, half_angle: *half_angle }
            }
            Surface::Sphere { frame, radius } => Surface::Sphere { frame: self.frame(frame)?, radius: *radius },
            Surface::BallPatch { center, radius } => Surface::BallPatch { center: self.point(*center), radius: *radius },
            Surface::Torus { frame, major_radius, minor_radius, tube_start } => {
                Surface::Torus { frame: self.frame(frame)?, major_radius: *major_radius, minor_radius: *minor_radius, tube_start: *tube_start }
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
            // Moving a definition is the kernel's: its supports, leader and seeds move with it.
            Surface::Blend { .. } => return None,
        })
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

/// This record moved by `motion` (module docs): topology, parameters and pcurves unchanged.
/// `None` for a record with a blend by its definition: moving its supports, leader and seeds is
/// the kernel's, which moves the definition with its net (its transform).
pub fn moved(brep: &Brep, motion: &Rigid) -> Option<Brep> {
    let mut out = brep.clone();
    for f in &mut out.faces {
        f.surface = motion.surface(&f.surface)?;
        f.nurbs = match &f.nurbs {
            Some(n) => Some(motion.surface(n)?),
            None => None,
        };
    }
    for e in &mut out.edges {
        e.curve = motion.curve(&e.curve)?;
        e.nurbs = match &e.nurbs {
            Some(n) => Some(motion.curve(n)?),
            None => None,
        };
    }
    for v in &mut out.vertices {
        v.point = motion.point(v.point);
    }
    Some(out)
}

/// A linear map that keeps handedness (rows of a matrix with positive determinant) and a
/// translation: `p ↦ linear · p + translation`.
struct Affine {
    linear: [[f64; 3]; 3],
    translation: [f64; 3],
}

impl Affine {
    fn direction(&self, d: [f64; 3]) -> [f64; 3] {
        self.linear.map(|row| row[0] * d[0] + row[1] * d[1] + row[2] * d[2])
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

    /// The curve mapped with its parameter, when its kind is one an affine map keeps whatever
    /// the map: lines, polylines and splines.
    fn curve(&self, c: &Curve) -> Option<Curve> {
        Some(match c {
            Curve::Line { start, end } => Curve::Line { start: self.point(*start), end: self.point(*end) },
            Curve::Polyline { points } => Curve::Polyline { points: points.iter().map(|p| self.point(*p)).collect() },
            Curve::Spline { knots, points } => Curve::Spline { knots: knots.clone(), points: points.iter().map(|p| self.point(*p)).collect() },
            Curve::RationalSpline { knots, homogeneous } => {
                Curve::RationalSpline { knots: knots.clone(), homogeneous: homogeneous.iter().map(|h| self.homogeneous(*h)).collect() }
            }
            Curve::SurfaceIntersection { surfaces, leader, tolerance } => Curve::SurfaceIntersection {
                surfaces: [Box::new(self.surface(&surfaces[0])?), Box::new(self.surface(&surfaces[1])?)],
                leader: Box::new(self.curve(leader)?),
                tolerance: tolerance * self.stretch(),
            },
            // A conic is a label on its NURBS form, which holds its parameter ([`Affine::conic`]).
            Curve::Circle { .. } | Curve::Ellipse { .. } => return None,
        })
    }

    /// The ellipse a circle or ellipse maps to, as a label: its parameter is its NURBS form's.
    /// `origin + p cos t + q sin t` is `origin + e₁ cos(t − t₀) + e₂ sin(t − t₀)` with the
    /// conjugate diameters e₁ and e₂ perpendicular at `tan 2t₀ = 2 p·q / (p·p − q·q)`.
    fn conic(&self, c: &Curve) -> Option<Curve> {
        let (frame, radii) = match c {
            Curve::Circle { frame, radius } => (frame, [*radius; 2]),
            Curve::Ellipse { frame, radii } => (frame, *radii),
            _ => return None,
        };
        let p = self.direction(frame.x_axis).map(|x| x * radii[0]);
        let q = self.direction(frame.y_axis).map(|x| x * radii[1]);
        let t0 = 0.5 * (2.0 * dot(p, q)).atan2(dot(p, p) - dot(q, q));
        let (sin, cos) = t0.sin_cos();
        let e1: [f64; 3] = std::array::from_fn(|i| cos * p[i] + sin * q[i]);
        let e2: [f64; 3] = std::array::from_fn(|i| cos * q[i] - sin * p[i]);
        let (r1, x_axis) = unit(e1)?;
        let along = dot(e2, x_axis);
        let (_, y_axis) = unit(std::array::from_fn(|i| e2[i] - along * x_axis[i]))?;
        let r2 = dot(e2, e2).sqrt();
        Some(Curve::Ellipse { frame: Frame { origin: self.point(frame.origin), x_axis, y_axis }, radii: [r1, r2] })
    }

    /// The surface mapped with its (u, v), when its kind is one an affine map keeps whatever the
    /// map: planes and splines.
    fn surface(&self, s: &Surface) -> Option<Surface> {
        Some(match s {
            Surface::Plane { origin, u_axis, v_axis } => {
                Surface::Plane { origin: self.point(*origin), u_axis: self.direction(*u_axis), v_axis: self.direction(*v_axis) }
            }
            Surface::PlaneThrough { origin, u_point, v_point } => {
                Surface::PlaneThrough { origin: self.point(*origin), u_point: self.point(*u_point), v_point: self.point(*v_point) }
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
            Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Sphere { .. }
            | Surface::BallPatch { .. }
            | Surface::Torus { .. }
            | Surface::PlacedRevolution { .. }
            | Surface::Blend { .. } => return None,
        })
    }

    /// At least how much the map lengthens any distance: √(‖L‖₁ ‖L‖∞) bounds the largest
    /// singular value, and is it for a diagonal map. Tolerances grow by this.
    fn stretch(&self) -> f64 {
        let l = &self.linear;
        let rows = (0..3).map(|i| l[i].iter().map(|x| x.abs()).sum::<f64>()).fold(0.0, f64::max);
        let columns = (0..3).map(|j| (0..3).map(|i| l[i][j].abs()).sum::<f64>()).fold(0.0, f64::max);
        (rows * columns).sqrt()
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The length of `v` and `v` over it, or `None` for a zero vector.
fn unit(v: [f64; 3]) -> Option<(f64, [f64; 3])> {
    let length = dot(v, v).sqrt();
    (length > 0.0 && length.is_finite()).then(|| (length, v.map(|x| x / length)))
}

/// This record mapped by a column-major affine matrix that keeps handedness (a scaling or shear,
/// rigid or not), topology, parameters and pcurves unchanged, its tolerances grown with it; or
/// `None` when the matrix reflects or is degenerate, or a face or edge is of a kind whose image
/// is not of that kind with the same parameters (the analytic surfaces other than planes). A
/// circle or ellipse becomes an ellipse, its NURBS form mapped with its parameter.
pub fn mapped(brep: &Brep, m: &[[f64; 4]; 4]) -> Option<Brep> {
    if m.iter().flatten().any(|x| !x.is_finite()) || m[0][3] != 0.0 || m[1][3] != 0.0 || m[2][3] != 0.0 || m[3][3] != 1.0 {
        return None;
    }
    let linear = [[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]];
    let [a, b, c] = linear;
    let det = a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]);
    if det.is_nan() || det <= 0.0 {
        return None;
    }
    let map = Affine { linear, translation: [m[3][0], m[3][1], m[3][2]] };
    let stretch = map.stretch();
    let mut out = brep.clone();
    out.tolerance *= stretch;
    for f in &mut out.faces {
        if f.nurbs.is_some() {
            return None;
        }
        f.surface = map.surface(&f.surface)?;
    }
    for e in &mut out.edges {
        match &e.nurbs {
            Some(nurbs) => {
                e.nurbs = Some(map.curve(nurbs)?);
                e.curve = match e.curve {
                    Curve::Circle { .. } | Curve::Ellipse { .. } => map.conic(&e.curve)?,
                    _ => map.curve(&e.curve)?,
                };
            }
            None => e.curve = map.curve(&e.curve)?,
        }
        e.tolerance *= stretch;
    }
    for v in &mut out.vertices {
        v.point = map.point(v.point);
        v.tolerance *= stretch;
    }
    Some(out)
}
