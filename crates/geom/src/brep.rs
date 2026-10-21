//! SolidCraft-owned portable B-rep data, independent of a modelling backend.
//! Millimetres, radians, right-handed Z-up. See docs/native-brep.md for semantics.
//! Validation checks structure and resource budgets, not geometric equivalence or trust.
//!
//! The records follow SMLib's flat form (`SmBrepData`): one record per logical use, the mated
//! halves a kernel keeps at run time left for it to rebuild. Names are STEP's B-rep names; each
//! type says its SMLib counterpart and STEP entity. Curves and surfaces are unbounded: an edge's
//! bounds are its `interval`, a face's its `domain` and loops.
use serde::{Deserialize, Serialize};

mod curve2;
pub mod envelope;
pub mod eval;
pub mod history;
pub mod pcurve;
mod topology;
pub use curve2::Curve2;

pub const VERSION: u32 = 4;
pub const MAX_ITEMS: usize = 1_000_000;
pub const MAX_SHELLS: usize = 1024;
const MAX_DEPTH: usize = 16;
const TAU: f64 = std::f64::consts::TAU;

/// A body (SMLib `SmBrep`, STEP `MANIFOLD_SOLID_BREP`/`BREP_WITH_VOIDS`/`SHELL_BASED_SURFACE_MODEL`).
/// Every list is in canonical order (docs/native-brep.md, "Canonical order"): a face's index
/// here is its face index everywhere, and so for edges and vertices.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Brep {
    pub version: u32,
    /// Intersection tolerance, mm (SMLib's per-brep `XSectTol3d`).
    pub tolerance: f64,
    /// `regions[0]` is the infinite region, then the bounded regions.
    pub regions: Vec<Region>,
    pub shells: Vec<Shell>,
    pub faces: Vec<Face>,
    pub edges: Vec<Edge>,
    pub vertices: Vec<Vertex>,
}

/// A connected volume of space (SMLib `SmRegionData`). The infinite region has no outer shell
/// and is never a void. A bounded region's `shells[0]` is its outer shell, a closed face shell;
/// the rest are inner shells. A closed shell separates two regions and is listed by both: as
/// the outer shell of the one it encloses and as an inner shell of the one around it. An open
/// shell, a wire shell or a vertex shell lies in one region and is listed by that one only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub shells: Vec<u32>,
    /// A bounded region that is empty space (a void), not material. False for the infinite region.
    pub void: bool,
}

/// A connected boundary component (SMLib `SmShellData`; STEP `CLOSED_SHELL`/`OPEN_SHELL`,
/// `WIRE_SHELL`, `VERTEX_SHELL`). One record stands for both runtime sides of a shell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shell {
    /// Connected faces, with wire edges embedded in them (`wires`, edges no face uses).
    Faces { faces: Vec<OrientedFace>, wires: Vec<u32> },
    /// A connected graph of wire edges and no faces.
    Wire { edges: Vec<u32> },
    /// One isolated point.
    Vertex { vertex: u32 },
}

/// A face as one shell uses it (SMLib `SmFaceuseData`, STEP `ORIENTED_FACE`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrientedFace {
    pub face: u32,
    /// The face's normal in this shell is its surface's natural normal (∂S/∂u × ∂S/∂v). In a
    /// closed shell the normals point out of the material.
    pub forward: bool,
}

/// A bounded piece of a surface (SMLib `SmFaceData`, STEP `ADVANCED_FACE`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Face {
    pub surface: Surface,
    /// The modelled rectangle in surface parameters, `[[u0, u1], [v0, v1]]`, inside the
    /// surface's natural domain and around every loop.
    pub domain: [[f64; 2]; 2],
    /// `loops[0]` is the outer loop, the rest inner. Loops are stored relative to the surface's
    /// natural normal, whatever side a shell uses: seen from that normal, the outer loop runs
    /// counter-clockwise and inner loops clockwise.
    pub loops: Vec<Loop>,
}

/// A face boundary (SMLib `SmLoopData`; STEP `FACE_BOUND` of an `EDGE_LOOP` or `VERTEX_LOOP`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Loop {
    /// A closed chain of oriented edges, in traversal order (STEP `EDGE_LOOP`).
    Edges { edges: Vec<OrientedEdge> },
    /// A one-vertex loop: a pole or a collapsed boundary (STEP `VERTEX_LOOP`). Inner only.
    Vertex { vertex: u32 },
}

/// One occurrence of an edge in a face's loop (SMLib `SmEUData`, one mated pair of runtime
/// edgeuses; STEP `ORIENTED_EDGE` with its `PCURVE`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrientedEdge {
    pub edge: u32,
    /// The loop runs along the edge from interval min (start vertex) to max (end vertex).
    pub forward: bool,
    /// The edge in this face's (u, v), on the edge's own interval and running min → max
    /// whatever `forward` says. A seam edge has two uses on one face, each with its pcurve.
    pub pcurve: Curve2,
}

/// A bounded piece of a curve (SMLib `SmEdgeData`, STEP `EDGE_CURVE`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub curve: Curve,
    /// The modelled interval in the curve's own parameter, min < max.
    pub interval: [f64; 2],
    /// `[start, end]`, at interval min and max; equal for a closed edge.
    pub vertices: [u32; 2],
    /// mm.
    pub tolerance: f64,
    /// Every oriented edge on this edge, each once, the primary use (the first in canonical
    /// order) first and the rest in radial order: right-hand rule about the edge's direction.
    /// Empty for a wire edge.
    pub uses: Vec<EdgeUseRef>,
}

/// Where one use of an edge sits (SMLib `SmEUData`'s radial fields): `faces[face]`,
/// `loops[loop]`, `edges[position]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeUseRef {
    pub face: u32,
    pub r#loop: u32,
    pub position: u32,
    /// Going round the edge, the radial cycle enters this use at the side on the surface's
    /// natural normal (SMLib's selected member of the mated pair). It follows from the loop
    /// convention: always `!forward` of the use it names.
    pub upper: bool,
}

/// A point (SMLib `SmVertexData`, STEP `VERTEX_POINT`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vertex {
    pub point: [f64; 3],
    /// mm.
    pub tolerance: f64,
}

/// Right-handed orthonormal frame; z = x_axis cross y_axis (STEP `AXIS2_PLACEMENT_3D`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub origin: [f64; 3],
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
}

/// Unbounded 3D curves, each with its parameter `t` (docs/native-brep.md, "Curves").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Curve {
    /// The line through both points: `start + t * (end - start)`, t = 0 at `start` and 1 at
    /// `end` (STEP `LINE`; truck's `Line`).
    Line { start: [f64; 3], end: [f64; 3] },
    /// `origin + radius * (cos t * x + sin t * y)`, t the angle from x (STEP `CIRCLE`).
    Circle { frame: Frame, radius: f64 },
    /// `origin + radii[0] cos t * x + radii[1] sin t * y` (STEP `ELLIPSE`).
    Ellipse { frame: Frame, radii: [f64; 2] },
    /// Straight segments; segment i spans t in [i, i + 1] (STEP `POLYLINE`).
    Polyline { points: Vec<[f64; 3]> },
    /// A B-spline on its knots (STEP `B_SPLINE_CURVE_WITH_KNOTS`).
    Spline { knots: Vec<f64>, points: Vec<[f64; 3]> },
    /// A NURBS curve: homogeneous `[x*w, y*w, z*w, w]` coefficients (STEP `RATIONAL_B_SPLINE_CURVE`).
    RationalSpline { knots: Vec<f64>, homogeneous: Vec<[f64; 4]> },
    /// The intersection of two surfaces, traced from `leader` (a polyline or spline near it)
    /// to within `tolerance`: the procedural curve itself, exactly as its producer holds it
    /// (STEP `INTERSECTION_CURVE`). t is the leader's parameter.
    SurfaceIntersection { surfaces: [Box<Surface>; 2], leader: Box<Curve>, tolerance: f64 },
}

/// Unbounded surfaces, each with its (u, v) (docs/native-brep.md, "Surfaces"). Natural
/// normals are ∂S/∂u × ∂S/∂v.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Surface {
    /// `origin + u * u_axis + v * v_axis` (STEP `PLANE`).
    Plane { origin: [f64; 3], u_axis: [f64; 3], v_axis: [f64; 3] },
    /// `origin + u * (u_point - origin) + v * (v_point - origin)`: the plane through three points,
    /// as its producer holds them (axes worked out from them would round; truck's `Plane`).
    PlaneThrough { origin: [f64; 3], u_point: [f64; 3], v_point: [f64; 3] },
    /// `origin + v z + radius (cos u x + sin u y)`: u the angle, v the height (STEP
    /// `CYLINDRICAL_SURFACE`).
    Cylinder { frame: Frame, radius: f64 },
    /// `origin + v z + r(v) (cos u x + sin u y)`, `r(v) = radius_at_origin + v tan(half_angle)`,
    /// on the side of the apex where r(v) ≥ 0: above it for a positive half angle, below it for
    /// a negative one (a cone narrowing along z, as SMLib makes them) (STEP `CONICAL_SURFACE`).
    Cone { frame: Frame, radius_at_origin: f64, half_angle: f64 },
    /// `origin + radius (cos v (cos u x + sin u y) + sin v z)`: u the longitude, v the latitude
    /// in [-π/2, π/2] (STEP `SPHERICAL_SURFACE`).
    Sphere { frame: Frame, radius: f64 },
    /// `origin + (R + r cos v)(cos u x + sin u y) + r sin v z`: u the longitude, v the meridian
    /// angle (STEP `TOROIDAL_SURFACE`).
    Torus { frame: Frame, major_radius: f64, minor_radius: f64 },
    /// A tensor-product B-spline: `points[i][j]`, i along u (STEP `B_SPLINE_SURFACE_WITH_KNOTS`).
    Spline { u_knots: Vec<f64>, v_knots: Vec<f64>, points: Vec<Vec<[f64; 3]>> },
    /// A NURBS surface: homogeneous `[x*w, y*w, z*w, w]` coefficients.
    RationalSpline { u_knots: Vec<f64>, v_knots: Vec<f64>, homogeneous: Vec<Vec<[f64; 4]>> },
    /// `profile` revolved about `origin` and unit `axis` (right-hand rule), then mapped by the
    /// column-major affine `matrix`: the revolution exactly as its producer holds it, nothing
    /// baked into the profile (truck's `RevolutedCurve` under a `Processor`). When `forward`,
    /// u is the profile's parameter and v the angle in [0, 2π); otherwise u and v swap (u the
    /// angle, v the profile's), which reverses the natural normal.
    PlacedRevolution { profile: Box<Curve>, origin: [f64; 3], axis: [f64; 3], matrix: [[f64; 4]; 4], forward: bool },
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
    require((2..=4096).contains(&count) && (1..=16).contains(&degree) && degree < count, "spline degree or size")?;
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
fn radius(r: f64) -> Result<()> {
    require(r.is_finite() && (1e-12..=1e12).contains(&r), "radius")
}
/// A tolerance in mm.
fn tolerance(t: f64) -> Result<()> {
    require(t.is_finite() && t > 0. && t <= 1e3, "tolerance")
}

/// A parameter's natural range: periodic with period 2π, or between two bounds (infinite for
/// an unbounded direction).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Natural {
    Periodic,
    Range(f64, f64),
}
const UNBOUNDED: Natural = Natural::Range(f64::NEG_INFINITY, f64::INFINITY);

/// The active interval of a spline's knots (already validated).
fn knot_domain(k: &[f64], count: usize) -> Natural {
    let degree = k.len().saturating_sub(count + 1);
    match (k.get(degree), k.get(count)) {
        (Some(a), Some(b)) => Natural::Range(*a, *b),
        _ => Natural::Range(0., 0.),
    }
}
fn curve_natural(c: &Curve) -> Natural {
    match c {
        Curve::Line { .. } => UNBOUNDED,
        Curve::Circle { .. } | Curve::Ellipse { .. } => Natural::Periodic,
        Curve::Polyline { points } => Natural::Range(0., points.len().saturating_sub(1) as f64),
        Curve::Spline { knots, points } => knot_domain(knots, points.len()),
        Curve::RationalSpline { knots, homogeneous } => knot_domain(knots, homogeneous.len()),
        Curve::SurfaceIntersection { leader, .. } => curve_natural(leader),
    }
}
fn surface_natural(s: &Surface) -> [Natural; 2] {
    use std::f64::consts::FRAC_PI_2;
    match s {
        Surface::Plane { .. } | Surface::PlaneThrough { .. } => [UNBOUNDED; 2],
        Surface::Cylinder { .. } => [Natural::Periodic, UNBOUNDED],
        Surface::Cone { radius_at_origin, half_angle, .. } => {
            let apex = -radius_at_origin / half_angle.tan();
            let v = if *half_angle > 0. { Natural::Range(apex, f64::INFINITY) } else { Natural::Range(f64::NEG_INFINITY, apex) };
            [Natural::Periodic, v]
        }
        Surface::Sphere { .. } => [Natural::Periodic, Natural::Range(-FRAC_PI_2, FRAC_PI_2)],
        Surface::Torus { .. } => [Natural::Periodic; 2],
        Surface::Spline { u_knots, v_knots, points } => {
            [knot_domain(u_knots, points.len()), knot_domain(v_knots, points.first().map_or(0, Vec::len))]
        }
        Surface::RationalSpline { u_knots, v_knots, homogeneous } => {
            [knot_domain(u_knots, homogeneous.len()), knot_domain(v_knots, homogeneous.first().map_or(0, Vec::len))]
        }
        Surface::PlacedRevolution { profile, forward: true, .. } => [curve_natural(profile), Natural::Periodic],
        Surface::PlacedRevolution { profile, forward: false, .. } => [Natural::Periodic, curve_natural(profile)],
    }
}
impl Curve {
    /// The range of `t`. Call on a validated record.
    pub fn natural(&self) -> Natural {
        curve_natural(self)
    }
}
impl Surface {
    /// The ranges of u and v. Call on a validated record.
    pub fn natural(&self) -> [Natural; 2] {
        surface_natural(self)
    }
}

/// `[min, max]`: finite, min < max, inside `natural` (at most one turn long where periodic).
fn interval(r: &[f64; 2], natural: Natural) -> Result<()> {
    numbers(*r)?;
    require(r[0] < r[1], "empty parameter interval")?;
    match natural {
        Natural::Periodic => {
            let slack = 1e-12 * (1. + r[0].abs().max(r[1].abs()));
            require(r[1] - r[0] <= TAU + slack, "parameter interval exceeds one turn")
        }
        Natural::Range(a, b) => require(a <= r[0] && r[1] <= b, "parameter interval outside the natural domain"),
    }
}

fn curve(c: &Curve, depth: usize, budget: &mut Budget) -> Result<()> {
    require(depth <= MAX_DEPTH, "geometry depth")?;
    budget.add(1)?;
    match c {
        Curve::Line { start, end } => {
            points(&[*start, *end], budget)?;
            require(start != end, "collapsed line")
        }
        Curve::Circle { frame: f, radius: r } => {
            frame(f, budget)?;
            radius(*r)
        }
        Curve::Ellipse { frame: f, radii } => {
            frame(f, budget)?;
            radii.iter().try_for_each(|r| radius(*r))
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
        Surface::Cylinder { frame: f, radius: r } | Surface::Sphere { frame: f, radius: r } => {
            frame(f, budget)?;
            radius(*r)
        }
        Surface::Cone { frame: f, radius_at_origin, half_angle } => {
            frame(f, budget)?;
            numbers([*radius_at_origin, *half_angle])?;
            require(*radius_at_origin >= 0. && half_angle.abs() >= 1e-12 && half_angle.abs() < std::f64::consts::FRAC_PI_2, "cone dimensions")
        }
        Surface::Torus { frame: f, major_radius, minor_radius } => {
            frame(f, budget)?;
            radius(*major_radius)?;
            radius(*minor_radius)?;
            require(major_radius > minor_radius, "ring torus radii")
        }
        Surface::Spline { u_knots, v_knots, points: p } => grid(u_knots, v_knots, p, budget),
        Surface::RationalSpline { u_knots, v_knots, homogeneous: p } => {
            grid(u_knots, v_knots, p, budget)?;
            for row in p {
                weights(row)?;
            }
            Ok(())
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
    /// Validate portable structure without calling a kernel or evaluating geometry: resource
    /// budgets, geometry records, parameter ranges, references, loops, regions, every edge's
    /// list of uses and the canonical order. Does not certify watertightness, that edges lie on
    /// their surfaces or that pcurves lift onto their edges.
    pub fn validate(&self) -> Result<()> {
        require(self.version == VERSION, "unsupported version")?;
        tolerance(self.tolerance)?;
        require(!self.shells.is_empty() && self.shells.len() <= MAX_SHELLS, "shell count")?;
        require(!self.regions.is_empty() && self.regions.len() <= 2 * MAX_SHELLS + 1, "region count")?;
        let mut budget = Budget(0);
        budget.add(self.regions.len() + self.faces.len() + self.edges.len())?;
        for v in &self.vertices {
            points(&[v.point], &mut budget)?;
            tolerance(v.tolerance)?;
        }
        for e in &self.edges {
            curve(&e.curve, 0, &mut budget)?;
            interval(&e.interval, curve_natural(&e.curve))?;
            require(e.vertices.iter().all(|&v| (v as usize) < self.vertices.len()), "edge vertices")?;
            tolerance(e.tolerance)?;
            budget.add(e.uses.len())?;
        }
        for f in &self.faces {
            surface(&f.surface, 0, &mut budget)?;
            let [u, v] = surface_natural(&f.surface);
            interval(&f.domain[0], u)?;
            interval(&f.domain[1], v)?;
            budget.add(f.loops.len())?;
            for l in &f.loops {
                if let Loop::Edges { edges } = l {
                    budget.add(edges.len())?;
                    for e in edges {
                        curve2::curve2(&e.pcurve, &mut budget)?;
                    }
                }
            }
        }
        topology::check(self)
    }
}

#[cfg(test)]
mod tests;
