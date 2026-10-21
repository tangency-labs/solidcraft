//! SolidCraft-owned portable B-rep data, independent of a modelling backend.
//! Millimetres, radians, right-handed Z-up. See docs/native-brep.md for semantics.
//! Validation checks structure and resource budgets, not geometric equivalence or trust.
use serde::{Deserialize, Serialize};

mod curve2;
pub mod envelope;
pub use curve2::Curve2;

pub const VERSION: u32 = 3;
pub const MAX_ITEMS: usize = 1_000_000;
const MAX_DEPTH: usize = 16;
/// Most control points along one direction of a spline (a helical thread flank written as one
/// spline has thousands along the helix); the item budget bounds a surface's grid as a whole.
pub const MAX_SPLINE_CONTROLS: usize = 8192;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Brep {
    pub version: u32,
    pub shells: Vec<Shell>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shell {
    pub vertices: Vec<[f64; 3]>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub vertices: [u32; 2],
    pub curve: Curve,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Face {
    pub loops: Vec<Vec<EdgeUse>>,
    pub surface: Surface,
    pub forward: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeUse {
    pub edge: u32,
    pub forward: bool,
}

/// Right-handed orthonormal frame; z = x_axis cross y_axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub origin: [f64; 3],
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Curve {
    Line {
        start: [f64; 3],
        end: [f64; 3],
    },
    CircleArc {
        frame: Frame,
        radius: f64,
        angle: [f64; 2],
    },
    EllipseArc {
        frame: Frame,
        radii: [f64; 2],
        angle: [f64; 2],
    },
    Polyline {
        points: Vec<[f64; 3]>,
    },
    Spline {
        knots: Vec<f64>,
        points: Vec<[f64; 3]>,
    },
    RationalSpline {
        knots: Vec<f64>,
        homogeneous: Vec<[f64; 4]>,
    },
    Intersection {
        knots: Vec<f64>,
        points: Vec<[f64; 3]>,
        fitting_tolerance: f64,
        faces: Option<[u32; 2]>,
    },
    /// The intersection of two surfaces, traced from `leader` (a polyline or spline near it)
    /// to within `tolerance`: the procedural curve itself, exactly as its producer holds it.
    SurfaceIntersection {
        surfaces: [Box<Surface>; 2],
        leader: Box<Curve>,
        tolerance: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Surface {
    Plane {
        origin: [f64; 3],
        u_axis: [f64; 3],
        v_axis: [f64; 3],
    },
    /// The plane through three points, parameter (1, 0) at `u_point` and (0, 1) at `v_point`:
    /// the points themselves, as its producer holds them (axes worked out from them would round).
    PlaneThrough {
        origin: [f64; 3],
        u_point: [f64; 3],
        v_point: [f64; 3],
    },
    Cylinder {
        frame: Frame,
        radius: f64,
        angle: [f64; 2],
        height: [f64; 2],
    },
    Cone {
        frame: Frame,
        radius_at_origin: f64,
        half_angle: f64,
        angle: [f64; 2],
        height: [f64; 2],
    },
    Sphere {
        frame: Frame,
        radius: f64,
        longitude: [f64; 2],
        latitude: [f64; 2],
    },
    Torus {
        frame: Frame,
        major_radius: f64,
        minor_radius: f64,
        longitude: [f64; 2],
        meridian: [f64; 2],
    },
    Spline {
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        points: Vec<Vec<[f64; 3]>>,
    },
    RationalSpline {
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        homogeneous: Vec<Vec<[f64; 4]>>,
    },
    Revolution {
        profile: Box<Curve>,
        origin: [f64; 3],
        axis: [f64; 3],
        angle: [f64; 2],
    },
    /// A full revolution of `profile` about `origin` and unit `axis`, then mapped by the
    /// column-major affine `matrix`, with the normal reversed unless `forward`: the
    /// revolution exactly as its producer holds it, nothing baked into the profile.
    PlacedRevolution {
        profile: Box<Curve>,
        origin: [f64; 3],
        axis: [f64; 3],
        matrix: [[f64; 4]; 4],
        forward: bool,
    },
}

#[derive(Clone, Debug, thiserror::Error)]
#[error("portable B-rep: {0}")]
pub struct Invalid(pub &'static str);

type Result<T> = std::result::Result<T, Invalid>;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(Invalid(message)) }
}

struct Budget(usize);
impl Budget {
    fn add(&mut self, n: usize) -> Result<()> {
        self.0 = self.0.checked_add(n).ok_or(Invalid("item budget"))?;
        require(self.0 <= MAX_ITEMS, "item budget")
    }
}
fn numbers(values: impl IntoIterator<Item = f64>) -> Result<()> {
    require(values.into_iter().all(|v| v.is_finite() && v.abs() <= 1e12), "non-finite or excessive coordinate")
}
fn points<const N: usize>(values: &[[f64; N]], budget: &mut Budget) -> Result<()> {
    budget.add(values.len())?;
    numbers(values.iter().flatten().copied())
}
fn knots(values: &[f64], count: usize, budget: &mut Budget) -> Result<()> {
    budget.add(values.len())?;
    numbers(values.iter().copied())?;
    let degree = values.len().checked_sub(count.saturating_add(1)).ok_or(Invalid("spline degree"))?;
    require((2..=MAX_SPLINE_CONTROLS).contains(&count) && (1..=16).contains(&degree) && degree < count, "spline degree or size")?;
    require(values.windows(2).all(|w| w.first() <= w.get(1)), "unordered knots")?;
    require(values.get(degree) < values.get(count), "empty spline domain")?;
    // No knot may have multiplicity greater than degree + 1.
    let mut run = 1;
    for pair in values.windows(2) {
        run = if pair.first() == pair.get(1) { run + 1 } else { 1 };
        require(run <= degree + 1, "knot multiplicity")?;
    }
    Ok(())
}
fn weights(values: &[[f64; 4]]) -> Result<()> {
    require(values.iter().all(|p| p.get(3).is_some_and(|w| *w >= 1e-12)), "non-positive rational weight")
}
fn grid<const N: usize>(u: &[f64], v: &[f64], rows: &[Vec<[f64; N]>], budget: &mut Budget) -> Result<()> {
    let columns = rows.first().map_or(0, Vec::len);
    knots(u, rows.len(), budget)?;
    knots(v, columns, budget)?;
    for row in rows {
        require(row.len() == columns, "ragged surface")?;
        points(row, budget)?;
    }
    Ok(())
}
fn vector(p: [f64; 3]) -> crate::Vec3 {
    crate::Vec3::new(p[0], p[1], p[2])
}
fn frame(f: &Frame, budget: &mut Budget) -> Result<()> {
    points(&[f.origin, f.x_axis, f.y_axis], budget)?;
    let (x, y) = (vector(f.x_axis), vector(f.y_axis));
    require((x.len() - 1.).abs() <= 1e-8 && (y.len() - 1.).abs() <= 1e-8 && x.dot(y).abs() <= 1e-8, "nonorthonormal frame")
}
fn range(r: &[f64; 2]) -> Result<()> {
    numbers(*r)?;
    require(r[0] < r[1], "empty parameter range")
}
fn angle(r: &[f64; 2]) -> Result<()> {
    range(r)?;
    require(r[1] - r[0] <= std::f64::consts::TAU + 1e-12, "angular range exceeds one turn")
}
fn radius(r: f64) -> Result<()> {
    require(r.is_finite() && (1e-12..=1e12).contains(&r), "radius")
}
fn curve(c: &Curve, depth: usize, budget: &mut Budget) -> Result<()> {
    require(depth <= MAX_DEPTH, "geometry depth")?;
    budget.add(1)?;
    match c {
        Curve::Line { start, end } => {
            points(&[*start, *end], budget)?;
            require(start != end, "collapsed line")
        }
        Curve::CircleArc { frame: f, radius: r, angle: a } => {
            frame(f, budget)?;
            radius(*r)?;
            angle(a)
        }
        Curve::EllipseArc { frame: f, radii, angle: a } => {
            frame(f, budget)?;
            for r in radii {
                radius(*r)?;
            }
            angle(a)
        }
        Curve::Polyline { points: p } => {
            require(p.len() >= 2, "empty polyline")?;
            points(p, budget)?;
            require(p.windows(2).all(|p| p.first() != p.get(1)), "collapsed polyline segment")
        }
        Curve::Spline { knots: k, points: p } => {
            knots(k, p.len(), budget)?;
            points(p, budget)
        }
        Curve::RationalSpline { knots: k, homogeneous: p } => {
            knots(k, p.len(), budget)?;
            points(p, budget)?;
            weights(p)
        }
        Curve::Intersection { knots: k, points: p, fitting_tolerance, .. } => {
            require((1e-12..=1.0).contains(fitting_tolerance), "intersection fitting tolerance")?;
            knots(k, p.len(), budget)?;
            points(p, budget)
        }
        Curve::SurfaceIntersection { surfaces, leader, tolerance } => {
            require(tolerance.is_finite() && (0.0..=1e3).contains(tolerance), "intersection tolerance")?;
            for s in surfaces {
                surface(s, depth + 1, budget)?;
            }
            // The leader is only a starting guess for the trace: its knots are the producer's,
            // and a traced polyline may repeat a point.
            require(depth < MAX_DEPTH, "geometry depth")?;
            budget.add(1)?;
            match &**leader {
                Curve::Polyline { points: p } => {
                    require(p.len() >= 2, "empty intersection leader")?;
                    points(p, budget)
                }
                Curve::Spline { knots: k, points: p } => {
                    knots(k, p.len(), budget)?;
                    points(p, budget)
                }
                _ => Err(Invalid("intersection leader")),
            }
        }
    }
}
fn surface(s: &Surface, depth: usize, budget: &mut Budget) -> Result<()> {
    require(depth <= MAX_DEPTH, "geometry depth")?;
    budget.add(1)?;
    match s {
        Surface::Plane { origin, u_axis, v_axis } => {
            points(&[*origin, *u_axis, *v_axis], budget)?;
            require(vector(*u_axis).cross(vector(*v_axis)).len() > 1e-12, "degenerate plane")
        }
        Surface::PlaneThrough { origin, u_point, v_point } => {
            points(&[*origin, *u_point, *v_point], budget)?;
            let o = vector(*origin);
            require((vector(*u_point) - o).cross(vector(*v_point) - o).len() > 1e-12, "degenerate plane")
        }
        Surface::Cylinder { frame: f, radius: r, angle: a, height } => {
            frame(f, budget)?;
            radius(*r)?;
            angle(a)?;
            range(height)
        }
        Surface::Cone { frame: f, radius_at_origin, half_angle, angle: a, height } => {
            frame(f, budget)?;
            numbers([*radius_at_origin, *half_angle])?;
            require(*radius_at_origin >= 0. && *half_angle > 0. && *half_angle < std::f64::consts::FRAC_PI_2, "cone dimensions")?;
            angle(a)?;
            range(height)?;
            require(height.iter().all(|h| radius_at_origin + h * half_angle.tan() >= 0.), "negative cone radius")
        }
        Surface::Sphere { frame: f, radius: r, longitude, latitude } => {
            frame(f, budget)?;
            radius(*r)?;
            angle(longitude)?;
            range(latitude)?;
            require(latitude[0] >= -std::f64::consts::FRAC_PI_2 && latitude[1] <= std::f64::consts::FRAC_PI_2, "sphere latitude")
        }
        Surface::Torus { frame: f, major_radius, minor_radius, longitude, meridian } => {
            frame(f, budget)?;
            radius(*major_radius)?;
            radius(*minor_radius)?;
            require(major_radius > minor_radius, "ring torus radii")?;
            angle(longitude)?;
            angle(meridian)
        }
        Surface::Spline { u_knots, v_knots, points: p } => grid(u_knots, v_knots, p, budget),
        Surface::RationalSpline { u_knots, v_knots, homogeneous: p } => {
            grid(u_knots, v_knots, p, budget)?;
            for row in p {
                weights(row)?;
            }
            Ok(())
        }
        Surface::Revolution { profile, origin, axis, angle: a } => {
            points(&[*origin, *axis], budget)?;
            require((vector(*axis).len() - 1.0).abs() <= 1e-8, "revolution axis is not unit length")?;
            angle(a)?;
            curve(profile, depth + 1, budget)
        }
        Surface::PlacedRevolution { profile, origin, axis, matrix, .. } => {
            points(&[*origin, *axis], budget)?;
            require((vector(*axis).len() - 1.0).abs() <= 1e-8, "revolution axis is not unit length")?;
            points(matrix, budget)?;
            let [x, y, z, _] = matrix.map(|c| vector([c[0], c[1], c[2]]));
            require(x.cross(y).dot(z).abs() > 1e-12, "singular revolution placement")?;
            curve(profile, depth + 1, budget)
        }
    }
}

impl Brep {
    /// Validate portable structure without calling a kernel or evaluating geometry.
    /// Does not certify watertightness, surface coincidence, or authoritative state.
    pub fn validate(&self) -> Result<()> {
        require(self.version == VERSION, "unsupported version")?;
        require(!self.shells.is_empty() && self.shells.len() <= 1024, "shell count")?;
        let mut budget = Budget(0);
        for shell in &self.shells {
            points(&shell.vertices, &mut budget)?;
            budget.add(shell.edges.len())?;
            budget.add(shell.faces.len())?;
            require(!shell.faces.is_empty(), "empty shell")?;
            for edge in &shell.edges {
                require(edge.vertices.iter().all(|&v| (v as usize) < shell.vertices.len()), "edge vertices")?;
                curve(&edge.curve, 0, &mut budget)?;
                if let Curve::Intersection { faces: Some(hints), .. } = &edge.curve {
                    require(hints.iter().all(|&i| (i as usize) < shell.faces.len()), "intersection face hint")?;
                }
            }
            for face in &shell.faces {
                require(!face.loops.is_empty(), "face loops")?;
                surface(&face.surface, 0, &mut budget)?;
                for wire in &face.loops {
                    require(!wire.is_empty(), "empty wire")?;
                    budget.add(wire.len())?;
                    let mut first = None;
                    let mut last = None;
                    for use_ in wire {
                        let edge = shell.edges.get(use_.edge as usize).ok_or(Invalid("edge reference"))?;
                        let [a, b] = edge.vertices;
                        let (start, end) = if use_.forward { (a, b) } else { (b, a) };
                        require(last.is_none_or(|v| v == start), "disconnected wire")?;
                        first.get_or_insert(start);
                        last = Some(end);
                    }
                    require(first == last, "open wire")?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn triangle() -> Brep {
        let vertices = vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let edges = (0..3)
            .map(|i| Edge { vertices: [i as u32, ((i + 1) % 3) as u32], curve: Curve::Line { start: vertices[i], end: vertices[(i + 1) % 3] } })
            .collect();
        Brep {
            version: VERSION,
            shells: vec![Shell {
                vertices,
                edges,
                faces: vec![Face {
                    loops: vec![(0..3).map(|edge| EdgeUse { edge, forward: true }).collect()],
                    surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
                    forward: true,
                }],
            }],
        }
    }
    #[test]
    fn portable_open_face_round_trip_without_a_kernel() {
        let body = triangle();
        body.validate().unwrap();
        let bytes = serde_json::to_vec(&body).unwrap();
        let restored: Brep = serde_json::from_slice(&bytes).unwrap();
        restored.validate().unwrap();
        assert_eq!(body, restored);
        assert!(!String::from_utf8(bytes).unwrap().contains("truck"));
    }
    #[test]
    fn topology_references_and_wire_orientation_are_checked() {
        let mut b = triangle();
        b.shells[0].faces[0].loops[0][0].edge = u32::MAX;
        assert!(b.validate().is_err());
        let mut b = triangle();
        b.shells[0].faces[0].loops[0][1].forward = false;
        assert!(b.validate().is_err());
        let mut b = triangle();
        b.shells[0].edges[0].vertices[0] = u32::MAX;
        assert!(b.validate().is_err());
        let mut b = triangle();
        b.version += 1;
        assert!(b.validate().is_err());
        let mut b = triangle();
        b.shells[0].vertices[0][0] = f64::NAN;
        assert!(b.validate().is_err());
    }
    #[test]
    fn closed_edge_can_share_its_endpoint_vertex() {
        let mut body = triangle();
        body.shells[0].vertices.truncate(1);
        body.shells[0].edges =
            vec![Edge { vertices: [0, 0], curve: Curve::Polyline { points: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 0.]] } }];
        body.shells[0].faces[0].loops = vec![vec![EdgeUse { edge: 0, forward: true }]];
        body.validate().unwrap();
    }

    #[test]
    fn spline_domains_weights_and_nesting_are_bounded() {
        let mut budget = Budget(0);
        assert!(knots(&[0., 0., 1., 1.], 2, &mut budget).is_ok());
        for bad in [vec![0., 0., 0., 0.], vec![0., 1., 0., 1.], vec![0., 0., f64::INFINITY, 1.]] {
            assert!(knots(&bad, 2, &mut Budget(0)).is_err());
        }
        let c = Curve::RationalSpline { knots: vec![0., 0., 1., 1.], homogeneous: vec![[0., 0., 0., 0.], [1., 0., 0., 1.]] };
        assert!(curve(&c, 0, &mut Budget(0)).is_err());
        let p = Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [2., 0., 0.] };
        assert!(surface(&p, 0, &mut Budget(0)).is_err());
        assert!(curve(&Curve::Line { start: [0.; 3], end: [1.; 3] }, MAX_DEPTH + 1, &mut Budget(0)).is_err());
        assert!(Budget(MAX_ITEMS).add(1).is_err());
        // Control points along one direction: up to the cap, not one more (a clamped cubic).
        let clamped = |count: usize| {
            let mut k = vec![0.0; 4];
            k.extend((1..count - 3).map(|i| i as f64));
            k.extend([(count - 3) as f64; 4]);
            k
        };
        assert!(knots(&clamped(MAX_SPLINE_CONTROLS), MAX_SPLINE_CONTROLS, &mut Budget(0)).is_ok());
        assert!(knots(&clamped(MAX_SPLINE_CONTROLS + 1), MAX_SPLINE_CONTROLS + 1, &mut Budget(0)).is_err());
    }

    #[test]
    fn exact_procedural_records_are_validated_and_round_trip() {
        let plane = |z: f64| Box::new(Surface::Plane { origin: [0., 0., z], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] });
        let leader = Box::new(Curve::Polyline { points: vec![[0.; 3], [0.; 3], [1., 0., 0.]] });
        let c = Curve::SurfaceIntersection { surfaces: [plane(0.), plane(1.)], leader: leader.clone(), tolerance: 1e-6 };
        curve(&c, 0, &mut Budget(0)).unwrap();
        assert_eq!(serde_json::from_str::<Curve>(&serde_json::to_string(&c).unwrap()).unwrap(), c);
        let line = Box::new(Curve::Line { start: [0.; 3], end: [1.; 3] });
        for bad in [
            Curve::SurfaceIntersection { surfaces: [plane(0.), plane(1.)], leader: line.clone(), tolerance: 1e-6 },
            Curve::SurfaceIntersection { surfaces: [plane(0.), plane(1.)], leader: leader.clone(), tolerance: f64::NAN },
            Curve::SurfaceIntersection { surfaces: [plane(f64::INFINITY), plane(1.)], leader, tolerance: 1e-6 },
        ] {
            assert!(curve(&bad, 0, &mut Budget(0)).is_err());
        }
        let identity = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];
        let s = Surface::PlacedRevolution { profile: line.clone(), origin: [0.; 3], axis: [0., 0., 1.], matrix: identity, forward: false };
        surface(&s, 0, &mut Budget(0)).unwrap();
        assert_eq!(serde_json::from_str::<Surface>(&serde_json::to_string(&s).unwrap()).unwrap(), s);
        let mut flat = identity;
        flat[2] = [0.; 4];
        let bad = Surface::PlacedRevolution { profile: line, origin: [0.; 3], axis: [0., 0., 1.], matrix: flat, forward: true };
        assert!(surface(&bad, 0, &mut Budget(0)).is_err());
    }

    #[test]
    fn analytic_records_have_independent_frames_and_domains() {
        use std::f64::consts::{FRAC_PI_2, TAU};
        let f = Frame { origin: [1., 2., 3.], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] };
        let surfaces = [
            Surface::Cylinder { frame: f.clone(), radius: 2., angle: [0., TAU], height: [-1., 3.] },
            Surface::Cone { frame: f.clone(), radius_at_origin: 2., half_angle: 0.2, angle: [0., TAU], height: [0., 3.] },
            Surface::Sphere { frame: f.clone(), radius: 2., longitude: [0., TAU], latitude: [-FRAC_PI_2, FRAC_PI_2] },
            Surface::Torus { frame: f.clone(), major_radius: 4., minor_radius: 1., longitude: [0., TAU], meridian: [0., TAU] },
            Surface::Revolution {
                profile: Box::new(Curve::Line { start: [2., 0., 0.], end: [2., 0., 3.] }),
                origin: [0.; 3],
                axis: [0., 0., 1.],
                angle: [0., TAU],
            },
        ];
        for s in surfaces {
            surface(&s, 0, &mut Budget(0)).unwrap();
            let encoded = serde_json::to_string(&s).unwrap();
            assert_eq!(serde_json::from_str::<Surface>(&encoded).unwrap(), s);
            assert!(!encoded.contains("columns") && !encoded.contains("forward"));
        }
        for c in [
            Curve::CircleArc { frame: f.clone(), radius: 2., angle: [0., FRAC_PI_2] },
            Curve::EllipseArc { frame: f.clone(), radii: [2., 3.], angle: [0., TAU] },
            Curve::Intersection { knots: vec![0., 0., 1., 1.], points: vec![[0.; 3], [1.; 3]], fitting_tolerance: 1e-6, faces: None },
        ] {
            curve(&c, 0, &mut Budget(0)).unwrap();
            let encoded = serde_json::to_string(&c).unwrap();
            assert_eq!(serde_json::from_str::<Curve>(&encoded).unwrap(), c);
            assert!(!encoded.contains("leader") && !encoded.contains("surfaces"));
        }
        let mut bad = f;
        bad.y_axis = bad.x_axis;
        assert!(frame(&bad, &mut Budget(0)).is_err());
        assert!(angle(&[0., TAU + 0.1]).is_err());
        assert!(range(&[2., 1.]).is_err());
        assert!(radius(f64::NAN).is_err());
        let mut b = triangle();
        b.shells[0].edges[0].curve =
            Curve::Intersection { knots: vec![0., 0., 1., 1.], points: vec![[0.; 3], [1., 0., 0.]], fitting_tolerance: 1e-6, faces: Some([0, 1]) };
        assert!(b.validate().is_err());
    }
}
