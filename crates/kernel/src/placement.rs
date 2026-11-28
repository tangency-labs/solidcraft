//! A record moved by a rigid motion: a rotation and a translation. Every point, axis and control
//! point moves; every parameter, pcurve, tolerance, orientation and the canonical order stay as
//! they are, because a rigid motion keeps distances, angles and handedness. The moved record is
//! the same B-rep in other coordinates, exactly. A scaling or shear ([`mapped`]) keeps a record
//! of planes, splines, lines and conics the same way (a circle becomes an ellipse), its
//! tolerances grown; it changes the kinds or
//! parameters of the other geometry, and a reflection the orientation: those are a kernel
//! operation, not this.
use solidcore_record::brep::blend::{
    BlendDefinition, BlendDefinitionRecord, NurbsCurveRecord, NurbsSurfaceRecord, ProceduralBlendRecord, RevolutionRecord, SupportRecord,
};
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
            // The definition moves as the kernel moves it: its supports, leader and centre. A
            // procedural blend's bound takes in the motion's rounding, from its net ([`moved`]).
            Surface::Blend { definition } => Surface::Blend { definition: Box::new(self.blend(definition)?) },
        })
    }

    /// Whether the rotation is orthonormal to rounding: a motion [`moved`] moves blend
    /// definitions by (beyond that it leaves them to the kernel). Faces that are not blends by
    /// their definition move by any rigid motion.
    pub fn is_exact(&self) -> bool {
        self.exact()
    }

    fn stored_point_fits(&self, p: [f64; 3]) -> bool {
        fits_point(self.point(p))
    }

    /// Homogeneous controls as stored, `R·(xw, yw, zw) + w·t`, weight unchanged. The stored
    /// components, not the cartesian point: a weight above 1 can fail while the point does not.
    fn stored_homogeneous_fits(&self, h: [f64; 4]) -> bool {
        let [x, y, z, _] = self.homogeneous(h);
        fits(x) && fits(y) && fits(z)
    }

    /// Frame origin after the motion. Re-unitized axes stay unit, so they cannot newly fail the
    /// ceiling; [`Self::frame`] returning `None` is not a ceiling failure.
    fn frame_origin_fits(&self, frame: &Frame) -> bool {
        self.frame(frame).is_none_or(|frame| fits_point(frame.origin))
    }

    fn plane_fits(&self, origin: [f64; 3], u_axis: [f64; 3], v_axis: [f64; 3]) -> bool {
        let Ok([u_axis, v_axis]) = solidcore_ops::affine::rigid_plane_axes(&self.matrix(), u_axis, v_axis) else {
            return true;
        };
        self.stored_point_fits(origin) && u_axis.into_iter().chain(v_axis).all(fits)
    }

    fn plane_through_fits(&self, origin: [f64; 3], u_point: [f64; 3], v_point: [f64; 3]) -> bool {
        let Ok((origin, [u_axis, v_axis])) = solidcore_ops::affine::rigid_plane_through(&self.matrix(), origin, u_point, v_point) else {
            return true;
        };
        fits_point(origin) && u_axis.into_iter().chain(v_axis).all(fits)
    }

    /// The revolution's placement after this motion: the matrix entries `validate_built` reads.
    /// Profile, origin and axis are not moved.
    fn placement_fits(&self, matrix: &[[f64; 4]; 4]) -> bool {
        compose(&self.matrix(), matrix).into_iter().flatten().all(fits)
    }

    /// Numbers [`Self::curve`] stores that the record's `numbers` check sees. Knots, weights,
    /// radii and tolerances are unchanged.
    fn curve_fits(&self, c: &Curve) -> bool {
        match c {
            Curve::Line { start, end } => self.stored_point_fits(*start) && self.stored_point_fits(*end),
            Curve::Circle { frame, .. } | Curve::Ellipse { frame, .. } => self.frame_origin_fits(frame),
            Curve::Polyline { points } => points.iter().all(|p| self.stored_point_fits(*p)),
            Curve::Spline { points, .. } => points.iter().all(|p| self.stored_point_fits(*p)),
            Curve::RationalSpline { homogeneous, .. } => homogeneous.iter().all(|h| self.stored_homogeneous_fits(*h)),
            Curve::SurfaceIntersection { surfaces, leader, .. } => {
                self.surface_fits(&surfaces[0]) && self.surface_fits(&surfaces[1]) && self.curve_fits(leader)
            }
        }
    }

    /// Numbers [`Self::surface`] stores that the record's `numbers` check sees. A blend
    /// definition is finite-only, so its net is not under that ceiling.
    fn surface_fits(&self, s: &Surface) -> bool {
        match s {
            Surface::Plane { origin, u_axis, v_axis } => self.plane_fits(*origin, *u_axis, *v_axis),
            Surface::PlaneThrough { origin, u_point, v_point } => self.plane_through_fits(*origin, *u_point, *v_point),
            Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. } | Surface::Sphere { frame, .. } | Surface::Torus { frame, .. } => {
                self.frame_origin_fits(frame)
            }
            Surface::BallPatch { center, .. } => self.stored_point_fits(*center),
            Surface::Spline { points, .. } => points.iter().flatten().all(|p| self.stored_point_fits(*p)),
            Surface::RationalSpline { homogeneous, .. } => homogeneous.iter().flatten().all(|h| self.stored_homogeneous_fits(*h)),
            Surface::PlacedRevolution { matrix, .. } => self.placement_fits(matrix),
            Surface::Blend { .. } => true,
        }
    }

    fn exact(&self) -> bool {
        const ROUNDING: f64 = 16.0 * f64::EPSILON;
        let column = |j: usize| [self.rotation[0][j], self.rotation[1][j], self.rotation[2][j]];
        (0..3).all(|i| (0..3).all(|j| (dot(column(i), column(j)) - if i == j { 1.0 } else { 0.0 }).abs() <= ROUNDING))
    }

    /// A blend's definition moved: the supports, the leader and (an exact round's) the centre
    /// the class is read at. Seeds are on the supports' parameters, the radius law on the
    /// leader's, and offsets and sides along the normals, which move with the supports. `None`
    /// for a motion only near rigid or a definition of a form this does not know to move.
    fn blend(&self, d: &BlendDefinition) -> Option<BlendDefinition> {
        if !self.exact() {
            return None;
        }
        Some(match d {
            BlendDefinition::Procedural(r) => BlendDefinition::Procedural(Box::new(self.procedural(r)?)),
            BlendDefinition::ExactRound(r) => {
                let mut r = r.clone();
                r.supports = [self.support(&r.supports[0])?, self.support(&r.supports[1])?];
                r.leader = match &r.leader {
                    Some(leader) => Some(self.nurbs_curve(leader)?),
                    None => None,
                };
                r.inputs.centre = self.point(r.inputs.centre);
                BlendDefinition::ExactRound(r)
            }
        })
    }

    /// A procedural definition moved, its bound as it was ([`widened`] takes in the rounding).
    fn procedural(&self, d: &BlendDefinitionRecord) -> Option<BlendDefinitionRecord> {
        let mut d = d.clone();
        d.supports = [self.support(&d.supports[0])?, self.support(&d.supports[1])?];
        d.leader = self.nurbs_curve(&d.leader)?;
        Some(d)
    }

    /// The cone or cylinder moved. `None` when the moved axes are not a frame: the support
    /// then keeps no revolution and reopens from its label.
    fn revolution(&self, r: &RevolutionRecord) -> Option<RevolutionRecord> {
        let [x_axis, y_axis] = solidcore_ops::affine::orthonormal_axes(self.direction(r.x_axis), self.direction(r.y_axis)).ok()?;
        Some(RevolutionRecord { origin: self.point(r.origin), x_axis, y_axis, radius: r.radius, semi_angle_deg: r.semi_angle_deg })
    }

    fn support(&self, s: &SupportRecord) -> Option<SupportRecord> {
        Some(match s {
            SupportRecord::Nurbs { kind, form, revolution } => SupportRecord::Nurbs {
                kind: *kind,
                form: self.nurbs_surface(form)?,
                revolution: revolution.as_ref().and_then(|r| self.revolution(r)),
            },
            SupportRecord::Blend(b) => {
                let net = self.nurbs_surface(&b.net)?;
                let mut definition = self.procedural(&b.definition)?;
                definition.bound = widened(definition.bound, net_points(&net)?);
                SupportRecord::Blend(Box::new(ProceduralBlendRecord { definition, net }))
            }
        })
    }

    /// A control point as stored, `(w x, w y, w z)` with weight `w`, or `(x, y, z)`.
    fn stored(&self, p: [f64; 3], w: Option<f64>) -> [f64; 3] {
        match w {
            Some(w) => {
                let h = self.homogeneous([p[0], p[1], p[2], w]);
                [h[0], h[1], h[2]]
            }
            None => self.point(p),
        }
    }

    fn nurbs_surface(&self, f: &NurbsSurfaceRecord) -> Option<NurbsSurfaceRecord> {
        let weights = weights_of(f.weights.as_deref(), f.points.len())?;
        Some(NurbsSurfaceRecord { points: f.points.iter().enumerate().map(|(i, p)| self.stored(*p, weights(i))).collect(), ..f.clone() })
    }

    /// A curve in space (the leader); `None` for a planar one, which is in some other frame.
    fn nurbs_curve(&self, c: &NurbsCurveRecord) -> Option<NurbsCurveRecord> {
        if c.dim != 3 || !c.points_3d {
            return None;
        }
        let weights = weights_of(c.weights.as_deref(), c.points.len())?;
        Some(NurbsCurveRecord { points: c.points.iter().enumerate().map(|(i, p)| self.stored(*p, weights(i))).collect(), ..c.clone() })
    }
}

/// The weight of each of `count` control points, when there are weights; `None` when their
/// number is not the points'.
fn weights_of(weights: Option<&[f64]>, count: usize) -> Option<impl Fn(usize) -> Option<f64> + '_> {
    if weights.is_some_and(|w| w.len() != count) {
        return None;
    }
    Some(move |i: usize| weights.and_then(|w| w.get(i).copied()))
}

/// The points and weights of a net's control points (stored weighted when rational).
fn net_points(net: &NurbsSurfaceRecord) -> Option<Vec<([f64; 3], f64)>> {
    let weights = weights_of(net.weights.as_deref(), net.points.len())?;
    net.points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let w = weights(i).unwrap_or(1.0);
            (w > 0.0 && w.is_finite()).then(|| (p.map(|x| x / w), w))
        })
        .collect()
}

/// A procedural blend's bound after a rigid motion of its net, as the kernel widens it (its
/// `transform_rounding`): each homogeneous coordinate is three products, their sum and the
/// weighted translation (six operations) on coordinates of at most the moved net's size, divided
/// by the least weight.
fn widened(bound: f64, net: Vec<([f64; 3], f64)>) -> f64 {
    let size = net.iter().fold(0.0_f64, |a, (p, _)| a.max(dot(*p, *p).sqrt()));
    let (lo, hi) = net.iter().fold((f64::INFINITY, 0.0_f64), |(lo, hi), (_, w)| (lo.min(*w), hi.max(*w)));
    let nu = 6.0 * (f64::EPSILON / 2.0);
    bound + nu / (1.0 - nu) * (hi / lo) * size
}

/// The net of a face's NURBS form (its `nurbs`), as a definition's net is stored.
fn face_net(s: &Surface) -> Option<Vec<([f64; 3], f64)>> {
    match s {
        Surface::Spline { points, .. } => Some(points.iter().flatten().map(|p| (*p, 1.0)).collect()),
        Surface::RationalSpline { homogeneous, .. } => {
            homogeneous.iter().flatten().map(|h| (h[3] > 0.0 && h[3].is_finite()).then(|| ([h[0] / h[3], h[1] / h[3], h[2] / h[3]], h[3]))).collect()
        }
        _ => None,
    }
}

/// The ceiling `Brep::validate_built` applies to a stored coordinate: finite, absolute value at
/// most 1e12.
const COORDINATE_CEILING: f64 = 1e12;

fn fits(x: f64) -> bool {
    x.is_finite() && x.abs() <= COORDINATE_CEILING
}

fn fits_point(p: [f64; 3]) -> bool {
    p.into_iter().all(fits)
}

/// Whether every coordinate [`moved`] would store, that the record's `numbers` check sees, is
/// finite and at most 1e12 in absolute value. Does not clone the record. A move helper returning
/// `None` is not a failure here: [`moved`] does not store that record. Unchanged fields (knots,
/// weights, radii, angles, intervals) are not read again. A blend definition's net is not under
/// this ceiling; the blend face's own NURBS form is.
pub fn within_coordinate_ceiling(brep: &Brep, motion: &Rigid) -> bool {
    brep.vertices.iter().all(|v| motion.stored_point_fits(v.point))
        && brep.edges.iter().all(|e| motion.curve_fits(&e.curve) && e.nurbs.as_ref().is_none_or(|n| motion.curve_fits(n)))
        && brep.faces.iter().all(|f| motion.surface_fits(&f.surface) && f.nurbs.as_ref().is_none_or(|n| motion.surface_fits(n)))
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
/// A blend by its definition moves as the kernel moves it ([`Rigid::blend`]); `None` when the
/// rotation is only near orthonormal, for an edge whose curve is on a blend, and for a definition
/// of a form not known to move.
pub fn moved(brep: &Brep, motion: &Rigid) -> Option<Brep> {
    let mut out = brep.clone();
    for f in &mut out.faces {
        f.surface = motion.surface(&f.surface)?;
        f.nurbs = match &f.nurbs {
            Some(n) => Some(motion.surface(n)?),
            None => None,
        };
        // A procedural blend's net is the face's NURBS form: its bound takes in the rounding of
        // moving it, as the kernel's move does.
        if let Surface::Blend { definition } = &mut f.surface
            && let BlendDefinition::Procedural(d) = definition.as_mut()
        {
            d.bound = widened(d.bound, face_net(f.nurbs.as_ref()?)?);
        }
    }
    for e in &mut out.edges {
        // An edge on a blend by its definition is the kernel's: its curve's copy of the blend
        // keeps no net of its own to take in the rounding by.
        if matches!(&e.curve, Curve::SurfaceIntersection { surfaces, .. } if surfaces.iter().any(|s| matches!(**s, Surface::Blend { .. }))) {
            return None;
        }
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
