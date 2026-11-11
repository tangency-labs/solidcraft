//! Portable B-rep data, independent of a modelling backend: SolidCraft's native B-rep record.
//! Millimetres, radians, right-handed Z-up. See SolidCraft's docs/native-brep.md for semantics.
//! Validation checks structure and resource budgets only: counts, indices, finite numbers, knot
//! vectors, parameter ranges, topology and canonical order. It computes no geometry; the kernel
//! checks that frames are orthonormal, planes and placements not degenerate, and that the
//! geometry agrees with the topology. Neither certifies geometric validity or trust.
//!
//! The records follow SMLib's flat form (`SmBrepData`): one record per logical use, the mated
//! halves a kernel keeps at run time left for it to rebuild. Names are STEP's B-rep names; each
//! type says its SMLib counterpart and STEP entity. Curves and surfaces are unbounded: an edge's
//! bounds are its `interval`, a face's its `domain` and loops.
use serde::{Deserialize, Serialize};

mod curve2;
pub mod envelope;
pub mod history;
pub mod part;
mod topology;
pub use curve2::Curve2;

pub const VERSION: u32 = 4;
/// The most items (records, points, knots, uses) [`Brep::validate`] takes in a record that comes
/// from outside the kernel: a file, the control channel, pasted data. A record the kernel built
/// ([`Brep::validate_built`]) has no such cap.
pub const MAX_ITEMS: usize = 4_000_000;
pub const MAX_SHELLS: usize = 1024;
/// Maximum number of nested geometry edges, with the root at depth zero.
pub const MAX_DEPTH: usize = 16;
/// The highest spline degree a record may carry.
pub const MAX_SPLINE_DEGREE: usize = 16;
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
    Faces {
        faces: Vec<OrientedFace>,
        wires: Vec<u32>,
    },
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
    /// The face's normal in this shell is the natural normal (∂S/∂u × ∂S/∂v) of the surface its
    /// (u, v) are on, [`Face::parameters`]: its NURBS form when it has one. In a closed shell the
    /// normals point out of the material.
    pub forward: bool,
}

/// A bounded piece of a surface (SMLib `SmFaceData`, STEP `ADVANCED_FACE`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Face {
    /// What the face is: its surface as the producer names it (a cylinder, a NURBS…).
    pub surface: Surface,
    /// The NURBS form the face is parameterised on, when it differs from `surface`'s own
    /// parameters: a cylinder, cone, sphere, torus or placed revolution as SMLib holds it (every
    /// SMLib analytic surface is a B-spline surface, its trims in that NURBS's parameters), and a
    /// ball patch's own net. When given, `domain` and every pcurve are in this surface's (u, v),
    /// and `surface` only says what it is; it is a `Spline` or `RationalSpline` and `surface` is a
    /// cylinder, cone, sphere, ball patch, torus or placed revolution
    /// (docs/native-brep.md, "Surfaces").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nurbs: Option<Surface>,
    /// The modelled rectangle in surface parameters, `[[u0, u1], [v0, v1]]`, inside the
    /// surface's natural domain and around every loop.
    pub domain: [[f64; 2]; 2],
    /// One outer loop and any inner ones, in the producer's order (truck lists some holes
    /// first; SMLib's import puts the outer loop first, as SMLib requires). Loops are stored
    /// relative to the surface's natural normal, whatever side a shell uses: seen from that
    /// normal, the outer loop runs counter-clockwise and inner loops clockwise, which is what
    /// tells them apart. The first loop is an edge loop.
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
    /// What the edge is: its curve as the producer names it (a line, a circle…).
    pub curve: Curve,
    /// The NURBS form the edge is parameterised on, when it differs from `curve`'s own
    /// parameter: a circle or ellipse as SMLib holds it (every SMLib conic is a B-spline curve,
    /// the trims on it in that NURBS's parameter). When given, `interval` and the pcurves' parameter
    /// are this curve's, and `curve` only says what it is; it is a `Spline` or `RationalSpline` and
    /// `curve` is a line, circle or ellipse (docs/native-brep.md, "Curves").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nurbs: Option<Curve>,
    /// The modelled interval in the edge's parameter ([`Edge::parameters`]), min < max.
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

/// Right-handed orthonormal frame; z = `x_axis` × `y_axis` (STEP `AXIS2_PLACEMENT_3D`).
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
    Spline {
        knots: Vec<f64>,
        points: Vec<[f64; 3]>,
    },
    /// A NURBS curve: homogeneous `[x*w, y*w, z*w, w]` coefficients (STEP `RATIONAL_B_SPLINE_CURVE`).
    RationalSpline {
        knots: Vec<f64>,
        homogeneous: Vec<[f64; 4]>,
    },
    /// The intersection of two surfaces, traced from `leader` (a polyline or spline near it)
    /// to within `tolerance`: the procedural curve itself, exactly as its producer holds it
    /// (STEP `INTERSECTION_CURVE`). t is the leader's parameter.
    SurfaceIntersection {
        surfaces: [Box<Surface>; 2],
        leader: Box<Curve>,
        tolerance: f64,
    },
}

/// Unbounded surfaces, each with its (u, v) (docs/native-brep.md, "Surfaces"). Natural
/// normals are ∂S/∂u × ∂S/∂v.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Surface {
    /// `origin + u * u_axis + v * v_axis` (STEP `PLANE`).
    Plane {
        origin: [f64; 3],
        u_axis: [f64; 3],
        v_axis: [f64; 3],
    },
    /// `origin + u * (u_point - origin) + v * (v_point - origin)`: the plane through three points,
    /// as its producer holds them (axes worked out from them would round; truck's `Plane`).
    PlaneThrough {
        origin: [f64; 3],
        u_point: [f64; 3],
        v_point: [f64; 3],
    },
    /// `origin + v z + radius (cos u x + sin u y)`: u the angle, v the height (STEP
    /// `CYLINDRICAL_SURFACE`).
    Cylinder { frame: Frame, radius: f64 },
    /// `origin + v z + r(v) (cos u x + sin u y)`, `r(v) = radius_at_origin + v tan(half_angle)`,
    /// on the side of the apex where r(v) ≥ 0: above it for a positive half angle, below it for
    /// a negative one (a cone narrowing along z, as SMLib makes them) (STEP `CONICAL_SURFACE`).
    /// Its half angle is at least 10⁻¹² unless the face carries its NURBS form, which is then
    /// the face's geometry (a cone narrower than that, its apex past 10¹² radii).
    Cone {
        frame: Frame,
        radius_at_origin: f64,
        half_angle: f64,
    },
    /// `origin + radius (cos v (cos u x + sin u y) + sin v z)`: u the longitude, v the latitude
    /// in [-π/2, π/2] (STEP `SPHERICAL_SURFACE`).
    Sphere { frame: Frame, radius: f64 },
    /// `origin + (R + r cos w)(cos u x + sin u y) + r sin w z`, `w = v + tube_start`: u the
    /// longitude, v the meridian angle measured from `tube_start` (radians; 0, the outer equator,
    /// in STEP `TOROIDAL_SURFACE` and older records). The tube start is the tube's own reference,
    /// as the frame's x axis is the longitude's: a face's form starts its tube on a quarter line
    /// from there (`form`). A ring torus (`major_radius > minor_radius`) unless the face carries
    /// its NURBS form, which is then the face's geometry (a tube that reaches the axis).
    Torus {
        frame: Frame,
        major_radius: f64,
        minor_radius: f64,
        #[serde(default, skip_serializing_if = "is_zero")]
        tube_start: f64,
    },
    /// A rational patch on the sphere of `center` and `radius`: the corner of three equal rounds as
    /// SMLib holds it (`SmBallPatch`). It has no (u, v) of its own: the face's `nurbs` is the patch,
    /// and this says it lies on that sphere, whose offsets are exact and collapse at its centre.
    BallPatch { center: [f64; 3], radius: f64 },
    /// A tensor-product B-spline: `points[i][j]`, i along u (STEP `B_SPLINE_SURFACE_WITH_KNOTS`).
    Spline {
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        points: Vec<Vec<[f64; 3]>>,
    },
    /// A NURBS surface: homogeneous `[x*w, y*w, z*w, w]` coefficients.
    RationalSpline {
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        homogeneous: Vec<Vec<[f64; 4]>>,
    },
    /// `profile` revolved about `origin` and unit `axis` (right-hand rule), then mapped by the
    /// column-major affine `matrix`: the revolution exactly as its producer holds it, nothing
    /// baked into the profile (truck's `RevolutedCurve` under a `Processor`). When `forward`,
    /// u is the profile's parameter and v the angle in [0, 2π); otherwise u and v swap (u the
    /// angle, v the profile's), which reverses the natural normal.
    PlacedRevolution {
        profile: Box<Curve>,
        origin: [f64; 3],
        axis: [f64; 3],
        matrix: [[f64; 4]; 4],
        forward: bool,
    },
}

/// Why a record is refused.
#[derive(Clone, Debug, thiserror::Error)]
#[error("portable B-rep: {0}")]
pub struct Invalid(pub &'static str);

/// A record check's result.
pub type Result<T> = std::result::Result<T, Invalid>;

/// `Ok` if `condition` holds, else [`Invalid`] with `message`.
pub fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Invalid(message))
    }
}

/// The items counted so far and the most allowed, shared by the checks of one input.
#[derive(Debug)]
pub struct Budget {
    used: usize,
    limit: usize,
}
impl Budget {
    /// The budget of a record from outside the kernel: [`MAX_ITEMS`].
    pub fn input() -> Budget {
        Budget {
            used: 0,
            limit: MAX_ITEMS,
        }
    }
    fn add(&mut self, n: usize) -> Result<()> {
        self.used = self.used.checked_add(n).ok_or(Invalid("item budget"))?;
        require(self.used <= self.limit, "item budget")
    }
}
fn numbers(values: impl IntoIterator<Item = f64>) -> Result<()> {
    require(
        values.into_iter().all(|v| v.is_finite() && v.abs() <= 1e12),
        "non-finite or excessive coordinate",
    )
}
fn points<const N: usize>(values: &[[f64; N]], budget: &mut Budget) -> Result<()> {
    budget.add(values.len())?;
    numbers(values.iter().flatten().copied())
}
fn knots(values: &[f64], count: usize, budget: &mut Budget) -> Result<()> {
    budget.add(values.len())?;
    numbers(values.iter().copied())?;
    let degree = values
        .len()
        .checked_sub(count.saturating_add(1))
        .ok_or(Invalid("spline degree"))?;
    require(
        (2..=4096).contains(&count) && (1..=MAX_SPLINE_DEGREE).contains(&degree) && degree < count,
        "spline degree or size",
    )?;
    require(
        values.windows(2).all(|w| w.first() <= w.get(1)),
        "unordered knots",
    )?;
    require(
        values.get(degree) < values.get(count),
        "empty spline domain",
    )?;
    // No knot may have multiplicity greater than degree + 1.
    let mut run = 1;
    for pair in values.windows(2) {
        run = if pair.first() == pair.get(1) {
            run + 1
        } else {
            1
        };
        require(run <= degree + 1, "knot multiplicity")?;
    }
    Ok(())
}
fn weights(values: &[[f64; 4]]) -> Result<()> {
    require(
        values.iter().all(|p| p.get(3).is_some_and(|w| *w >= 1e-12)),
        "non-positive rational weight",
    )
}
fn grid<const N: usize>(
    u: &[f64],
    v: &[f64],
    rows: &[Vec<[f64; N]>],
    budget: &mut Budget,
) -> Result<()> {
    let columns = rows.first().map_or(0, Vec::len);
    knots(u, rows.len(), budget)?;
    knots(v, columns, budget)?;
    for row in rows {
        require(row.len() == columns, "ragged surface")?;
        points(row, budget)?;
    }
    Ok(())
}
/// A frame's numbers. That its axes are orthonormal is the kernel's check.
fn frame(f: &Frame, budget: &mut Budget) -> Result<()> {
    points(&[f.origin, f.x_axis, f.y_axis], budget)
}
/// A field left out of a record where it is zero (its default). By reference: serde's
/// `skip_serializing_if` passes one.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(x: &f64) -> bool {
    *x == 0.0
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
/// The natural range of an unbounded direction.
pub const UNBOUNDED: Natural = Natural::Range(f64::NEG_INFINITY, f64::INFINITY);

/// The active interval of a spline's knots (already validated): `[knots[degree], knots[count]]`
/// for `count` control points.
pub fn knot_domain(k: &[f64], count: usize) -> Natural {
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
/// The natural ranges of the (u, v) a face's domain and pcurves are on, [`Face::parameters`]:
/// a plane's or a spline's. Other surfaces are labels whose (u, v) are their NURBS form's, which
/// validation requires; their natural ranges are the kernel's (a cone's apex is computed).
fn parameter_natural(s: &Surface) -> Result<[Natural; 2]> {
    match s {
        Surface::Plane { .. } | Surface::PlaneThrough { .. } => Ok([UNBOUNDED; 2]),
        Surface::Spline {
            u_knots,
            v_knots,
            points,
        } => Ok([
            knot_domain(u_knots, points.len()),
            knot_domain(v_knots, points.first().map_or(0, Vec::len)),
        ]),
        Surface::RationalSpline {
            u_knots,
            v_knots,
            homogeneous,
        } => Ok([
            knot_domain(u_knots, homogeneous.len()),
            knot_domain(v_knots, homogeneous.first().map_or(0, Vec::len)),
        ]),
        _ => Err(Invalid("a face's parameters on a surface without its own")),
    }
}
impl Curve {
    /// The range of `t`. Call on a validated record.
    pub fn natural(&self) -> Natural {
        curve_natural(self)
    }
}

/// The structure of one curve record from outside the kernel, as [`Brep::validate`] checks an
/// edge's, counted against `budget`.
pub fn check_curve(c: &Curve, budget: &mut Budget) -> Result<()> {
    curve(c, 0, budget)
}

/// The structure of one surface record from outside the kernel, as [`Brep::validate`] checks a
/// face's, counted against `budget`.
pub fn check_surface(s: &Surface, budget: &mut Budget) -> Result<()> {
    surface(s, 0, budget)
}

/// The structure of one pcurve record from outside the kernel, as [`Brep::validate`] checks an
/// oriented edge's, counted against `budget`.
pub fn check_curve2(c: &Curve2, budget: &mut Budget) -> Result<()> {
    curve2::curve2(c, budget)
}

/// `[min, max]` is a parameter interval inside `natural`, as [`Brep::validate`] checks an edge's
/// and a face's: finite, min < max, and at most one turn long where periodic.
pub fn check_interval(r: &[f64; 2], natural: Natural) -> Result<()> {
    interval(r, natural)
}

/// `[min, max]`: finite, min < max, inside `natural` (at most one turn long where periodic).
fn interval(r: &[f64; 2], natural: Natural) -> Result<()> {
    numbers(*r)?;
    require(r[0] < r[1], "empty parameter interval")?;
    match natural {
        Natural::Periodic => {
            let slack = 1e-12 * (1. + r[0].abs().max(r[1].abs()));
            require(
                r[1] - r[0] <= TAU + slack,
                "parameter interval exceeds one turn",
            )
        }
        Natural::Range(a, b) => require(
            a <= r[0] && r[1] <= b,
            "parameter interval outside the natural domain",
        ),
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
        Curve::Circle {
            frame: f,
            radius: r,
        } => {
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
            require(
                p.windows(2).all(|p| p.first() != p.get(1)),
                "collapsed polyline segment",
            )
        }
        Curve::Spline {
            knots: k,
            points: p,
        } => {
            knots(k, p.len(), budget)?;
            points(p, budget)
        }
        Curve::RationalSpline {
            knots: k,
            homogeneous: p,
        } => {
            knots(k, p.len(), budget)?;
            points(p, budget)?;
            weights(p)
        }
        Curve::SurfaceIntersection {
            surfaces,
            leader,
            tolerance,
        } => {
            require(
                tolerance.is_finite() && (0.0..=1e3).contains(tolerance),
                "intersection tolerance",
            )?;
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
                Curve::Spline {
                    knots: k,
                    points: p,
                } => {
                    knots(k, p.len(), budget)?;
                    points(p, budget)
                }
                _ => Err(Invalid("intersection leader")),
            }
        }
    }
}
/// A face's own surface: as [`surface`], except that a cone or torus label, which a face always
/// carries with its NURBS form (the form is the face's geometry and the label a proved name for
/// it), may be degenerate: a cone of half angle under 10⁻¹² (its apex past 10¹² radii) and a
/// torus whose tube reaches its axis (`major_radius <= minor_radius`). A bare cone or torus,
/// with no form to compute from, keeps [`surface`]'s limits.
fn face_label(s: &Surface, budget: &mut Budget) -> Result<()> {
    match s {
        Surface::Cone {
            frame: f,
            radius_at_origin,
            half_angle,
        } => {
            budget.add(1)?;
            frame(f, budget)?;
            numbers([*radius_at_origin, *half_angle])?;
            require(
                *radius_at_origin >= 0. && half_angle.abs() < std::f64::consts::FRAC_PI_2,
                "cone dimensions",
            )
        }
        Surface::Torus {
            frame: f,
            major_radius,
            minor_radius,
            tube_start,
        } => {
            budget.add(1)?;
            frame(f, budget)?;
            radius(*major_radius)?;
            radius(*minor_radius)?;
            require(
                tube_start.is_finite() && tube_start.abs() <= std::f64::consts::TAU,
                "torus tube start",
            )
        }
        _ => surface(s, 0, budget),
    }
}

fn surface(s: &Surface, depth: usize, budget: &mut Budget) -> Result<()> {
    require(depth <= MAX_DEPTH, "geometry depth")?;
    budget.add(1)?;
    match s {
        Surface::Plane {
            origin,
            u_axis,
            v_axis,
        } => {
            points(&[*origin, *u_axis, *v_axis], budget)?;
            Ok(())
        }
        Surface::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => points(&[*origin, *u_point, *v_point], budget),
        Surface::Cylinder {
            frame: f,
            radius: r,
        }
        | Surface::Sphere {
            frame: f,
            radius: r,
        } => {
            frame(f, budget)?;
            radius(*r)
        }
        Surface::Cone {
            frame: f,
            radius_at_origin,
            half_angle,
        } => {
            frame(f, budget)?;
            numbers([*radius_at_origin, *half_angle])?;
            require(
                *radius_at_origin >= 0.
                    && half_angle.abs() >= 1e-12
                    && half_angle.abs() < std::f64::consts::FRAC_PI_2,
                "cone dimensions",
            )
        }
        Surface::Torus {
            frame: f,
            major_radius,
            minor_radius,
            tube_start,
        } => {
            frame(f, budget)?;
            radius(*major_radius)?;
            radius(*minor_radius)?;
            require(
                tube_start.is_finite() && tube_start.abs() <= std::f64::consts::TAU,
                "torus tube start",
            )?;
            require(major_radius > minor_radius, "ring torus radii")
        }
        Surface::BallPatch { center, radius: r } => {
            points(&[*center], budget)?;
            radius(*r)
        }
        Surface::Spline {
            u_knots,
            v_knots,
            points: p,
        } => grid(u_knots, v_knots, p, budget),
        Surface::RationalSpline {
            u_knots,
            v_knots,
            homogeneous: p,
        } => {
            grid(u_knots, v_knots, p, budget)?;
            for row in p {
                weights(row)?;
            }
            Ok(())
        }
        Surface::PlacedRevolution {
            profile,
            origin,
            axis,
            matrix,
            ..
        } => {
            points(&[*origin, *axis], budget)?;
            points(matrix, budget)?;
            curve(profile, depth + 1, budget)
        }
    }
}

impl Edge {
    /// The curve the edge's parameter is on: its NURBS form when it has one, else its curve.
    /// Everything that evaluates the edge at a parameter goes through this, with `interval`.
    pub fn parameters(&self) -> &Curve {
        self.nurbs.as_ref().unwrap_or(&self.curve)
    }
}

impl Face {
    /// The surface the face's (u, v) are on: its NURBS form when it has one, else its surface.
    /// Everything that evaluates the face's domain or pcurves goes through this.
    pub fn parameters(&self) -> &Surface {
        self.nurbs.as_ref().unwrap_or(&self.surface)
    }
}

impl Brep {
    /// Validate portable structure without calling a kernel or evaluating geometry: resource
    /// budgets, geometry records, parameter ranges, references, loops, regions, every edge's
    /// list of uses and the canonical order. Computes nothing: that frames are orthonormal and
    /// planes and placements not degenerate, watertightness, that edges lie on their surfaces and
    /// that pcurves lift onto their edges are the kernel's checks.
    ///
    /// For a record from outside the kernel (a file, the control channel, pasted data): it is
    /// refused past [`MAX_ITEMS`] items, so hostile input is bounded before anything walks it.
    pub fn validate(&self) -> Result<()> {
        self.validate_within(Budget::input())
    }

    /// [`Brep::validate`] for a record the kernel built in memory (an operation's result, a
    /// body's own record, a part of a validated record): every check but the item budget. The
    /// record already sits in memory, its size set by inputs that were bounded where they came
    /// in, and the checks are linear in it; a cap here would only refuse a real result.
    pub fn validate_built(&self) -> Result<()> {
        self.validate_within(Budget {
            used: 0,
            limit: usize::MAX,
        })
    }

    fn validate_within(&self, mut budget: Budget) -> Result<()> {
        require(self.version == VERSION, "unsupported version")?;
        tolerance(self.tolerance)?;
        require(
            !self.shells.is_empty() && self.shells.len() <= MAX_SHELLS,
            "shell count",
        )?;
        require(
            !self.regions.is_empty() && self.regions.len() <= 2 * MAX_SHELLS + 1,
            "region count",
        )?;
        budget.add(self.regions.len() + self.faces.len() + self.edges.len())?;
        for v in &self.vertices {
            points(&[v.point], &mut budget)?;
            tolerance(v.tolerance)?;
        }
        for e in &self.edges {
            curve(&e.curve, 0, &mut budget)?;
            if let Some(n) = &e.nurbs {
                curve(n, 0, &mut budget)?;
                require(
                    matches!(n, Curve::Spline { .. } | Curve::RationalSpline { .. }),
                    "NURBS form is not a NURBS",
                )?;
                require(
                    matches!(
                        e.curve,
                        Curve::Line { .. } | Curve::Circle { .. } | Curve::Ellipse { .. }
                    ),
                    "NURBS form on a curve that is not a line, circle or ellipse",
                )?;
            }
            // A circle or ellipse is a label: its geometry is its NURBS form.
            require(
                e.nurbs.is_some()
                    || !matches!(e.curve, Curve::Circle { .. } | Curve::Ellipse { .. }),
                "a circle or ellipse without its NURBS form",
            )?;
            interval(&e.interval, curve_natural(e.parameters()))?;
            require(
                e.vertices
                    .iter()
                    .all(|&v| (v as usize) < self.vertices.len()),
                "edge vertices",
            )?;
            tolerance(e.tolerance)?;
            budget.add(e.uses.len())?;
        }
        for f in &self.faces {
            if f.nurbs.is_some() {
                face_label(&f.surface, &mut budget)?;
            } else {
                surface(&f.surface, 0, &mut budget)?;
            }
            if let Some(n) = &f.nurbs {
                surface(n, 0, &mut budget)?;
                require(
                    matches!(n, Surface::Spline { .. } | Surface::RationalSpline { .. }),
                    "NURBS form is not a NURBS",
                )?;
                require(
                    matches!(
                        f.surface,
                        Surface::Cylinder { .. }
                            | Surface::Cone { .. }
                            | Surface::Sphere { .. }
                            | Surface::BallPatch { .. }
                            | Surface::Torus { .. }
                            | Surface::PlacedRevolution { .. }
                    ),
                    "NURBS form on a surface that is not a cylinder, cone, sphere, ball patch, torus or revolution",
                )?;
            }
            // A cylinder, cone, sphere, ball patch, torus or revolution is a label: its geometry is
            // its form.
            require(
                f.nurbs.is_some()
                    || !matches!(
                        f.surface,
                        Surface::Cylinder { .. }
                            | Surface::Cone { .. }
                            | Surface::Sphere { .. }
                            | Surface::BallPatch { .. }
                            | Surface::Torus { .. }
                            | Surface::PlacedRevolution { .. }
                    ),
                "a cylinder, cone, sphere, ball patch, torus or revolution without its NURBS form",
            )?;
            let [u, v] = parameter_natural(f.parameters())?;
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
