//! Planar prisms: booleans of planar regions, the extrusion of planar material, whether a body
//! is a uniform prism (its section) or the same prism as another, edges read in a plane, and a
//! view's points in its plane and points along a line on it, as a drawing reads them.
//!
//! Each is a recipe of kernel calls (`solidcore::prism`): the kernel checks the profiles, splits
//! their loops, makes their curves, reads sheets back as regions, extrudes a region and proves a
//! prism; ops sequences those with the kernel's planar faces and 2D boolean, the record's
//! planar profiles and planes converted by `solidcore-convert` ([`convert::region2`],
//! [`convert::plane`] and back). It computes no geometric quantity.
//!
//! The booleans keep free-form curves exact: every boundary of a result is a piece of an
//! operand's boundary, cut where it crosses the other, never refitted.

// The range tests refuse NaN by negation (`!(x > y)`), kept as written.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use solidcore::boolean::api_brep::{boolean_2d, Boolean2dOutcome};
use solidcore::eval::Frame;
use solidcore::kernel::Kernel;
use solidcore::primitive::api_trim_ops::create_planar_faces;
use solidcore::primitive::primitive_creation::Boolean2d;
use solidcore::prism::{extrude, measure, profile as kp, read, sheet};
use solidcore::Brep;
use solidcore_convert as convert;
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

pub use solidcore::prism::measure::Interval;
pub use solidcore::prism::profile::{tapered_flange_ends, ProfileRefusal};

use crate::body::{finish, Body, Expect};
use crate::boolean::{union_all, BooleanOp};
use crate::order::{self, Order};
use crate::session::{read_body, run, run_on, Answer};
use crate::{Error, Result};

/// The tolerance (mm) the planar booleans take: at least this.
pub const MIN_TOLERANCE: f64 = 1e-9;
/// ... and at most this.
pub const MAX_TOLERANCE: f64 = 1.0;
/// The tolerance the planar extrusion closes and splits its loops with (mm).
pub const EXTRUDE_TOLERANCE: f64 = 1e-7;

/// A call's own refusal, else its value, as a kernel call answers inside [`run`].
struct Out<T>(std::result::Result<T, Error>);

impl<T> Answer for Out<T> {
    fn refusal(&self) -> Option<String> {
        self.0.as_ref().err().map(ToString::to_string)
    }
}

/// The frame the planar booleans work in: the regions' own coordinates.
fn regions_frame() -> Frame {
    convert::plane(&Plane::XY)
}

fn op2(op: BooleanOp) -> Boolean2d {
    match op {
        BooleanOp::Union => Boolean2d::Union,
        BooleanOp::Subtract => Boolean2d::Difference,
        BooleanOp::Intersect => Boolean2d::Intersection,
    }
}

/// `a` op `b` (the kernel's 2D boolean); `None` when nothing is left.
fn combine(k: &mut Kernel, a: Brep, b: Brep, op: BooleanOp) -> solidcore::Result<Option<Brep>> {
    Ok(match boolean_2d(k, a, b, op2(op))? {
        Boolean2dOutcome::Result(r) => Some(r),
        Boolean2dOutcome::Unassigned { .. } => None,
    })
}

/// `sheets` united in balanced rounds (each union of two sheets of about the same size);
/// `None` for none.
fn united(
    k: &mut Kernel,
    mut sheets: Vec<Brep>,
) -> solidcore::Result<std::result::Result<Option<Brep>, Error>> {
    while sheets.len() > 1 {
        let mut next = Vec::new();
        let mut iter = sheets.into_iter();
        while let Some(a) = iter.next() {
            next.push(match iter.next() {
                Some(b) => match combine(k, a, b, BooleanOp::Union)? {
                    Some(u) => u,
                    None => return Ok(Err(Error::Unsupported("a union of planar faces is empty"))),
                },
                None => a,
            });
        }
        sheets = next;
    }
    Ok(Ok(sheets.pop()))
}

/// Each of `loops` one planar face (the kernel's curves on the regions' frame, its planar
/// faces).
fn faces(k: &mut Kernel, loops: &[kp::Loop2]) -> solidcore::Result<Vec<Brep>> {
    let f = regions_frame();
    loops
        .iter()
        .map(|lp| {
            let curves = lp
                .segs
                .iter()
                .map(|s| kp::curve_on_frame(k, &f, 0.0, s))
                .collect::<solidcore::Result<Vec<_>>>()?;
            create_planar_faces(k, &curves)
        })
        .collect()
}

/// `regions` as one sheet of planar faces on the regions' frame: each region's material united
/// less its holes united (the kernel's simple loops of it, [`sheet::region_loops`]), the regions
/// united. A region may lie in another's hole (an island in a ring), which the other's hole must
/// not take: so each region is made on its own first.
fn sheet_of(
    k: &mut Kernel,
    regions: &[kp::Region2],
    tol: f64,
) -> solidcore::Result<std::result::Result<Brep, Error>> {
    let mut sheets = Vec::new();
    for r in regions {
        let loops = sheet::region_loops(r, tol)?;
        let material = faces(k, &loops.material)?;
        let Some(material) = (match united(k, material)? {
            Ok(m) => m,
            Err(e) => return Ok(Err(e)),
        }) else {
            continue;
        };
        let holes = faces(k, &loops.holes)?;
        sheets.push(match united(k, holes)? {
            Ok(Some(holes)) => match combine(k, material, holes, BooleanOp::Subtract)? {
                Some(s) => s,
                None => return Ok(Err(Error::Unsupported("the holes take all the material"))),
            },
            Ok(None) => material,
            Err(e) => return Ok(Err(e)),
        });
    }
    Ok(match united(k, sheets)? {
        Ok(Some(s)) => Ok(s),
        Ok(None) => Err(Error::Empty),
        Err(e) => Err(e),
    })
}

/// The profile check's refusal as the call's.
fn checked<'a>(
    regions: impl Iterator<Item = &'a kp::Region2>,
    tol: f64,
) -> std::result::Result<(), Error> {
    kp::check_regions(regions, tol).map_err(Error::Profile)
}

/// The boolean of planar regions bounded by lines, circular arcs and free-form segments, exact:
/// every boundary of the result is a piece of a boundary of `a` or `b`, cut where it crosses the
/// other. Disjoint results and retained islands are separate regions. `tolerance` (mm,
/// [`MIN_TOLERANCE`] to [`MAX_TOLERANCE`]) is how far apart a loop's segments may end and still
/// close it.
///
/// # Errors
/// [`Error::Argument`] for a tolerance out of range; [`Error::Profile`] for a profile the kernel
/// refuses; [`Error::Unsupported`] for a sheet the kernel cannot read back exactly; the kernel's
/// error.
pub fn planar_regions_boolean(
    a: &[Region2],
    b: &[Region2],
    op: BooleanOp,
    tolerance: f64,
) -> Result<Vec<Region2>> {
    if !(tolerance.is_finite() && (MIN_TOLERANCE..=MAX_TOLERANCE).contains(&tolerance)) {
        return Err(Error::Argument);
    }
    let (ka, kb): (Vec<_>, Vec<_>) = (
        a.iter().map(convert::region2).collect(),
        b.iter().map(convert::region2).collect(),
    );
    checked(ka.iter().chain(&kb), tolerance)?;
    match (a.is_empty(), b.is_empty(), op) {
        (true, _, BooleanOp::Union) => return Ok(b.to_vec()),
        (true, _, _) | (false, true, BooleanOp::Intersect) => return Ok(Vec::new()),
        (false, true, _) => return Ok(a.to_vec()),
        _ => {}
    }
    let out = run("planar_regions_boolean", |k| {
        let sa = match sheet_of(k, &ka, tolerance)? {
            Ok(s) => s,
            Err(e) => return Ok(Out(Err(e))),
        };
        let sb = match sheet_of(k, &kb, tolerance)? {
            Ok(s) => s,
            Err(e) => return Ok(Out(Err(e))),
        };
        let Some(r) = combine(k, sa, sb, op)? else {
            return Ok(Out(Ok(Vec::new())));
        };
        let read = sheet::sheet_regions(k, &r, &regions_frame(), tolerance)?;
        Ok(Out(Ok(read.regions)))
    })?;
    Ok(out.0?.iter().map(convert::record_region2).collect())
}

/// Normalized planar material extruded as one body, keeping every disjoint region and its
/// holes: the regions united in the plane first (overlapping ones merge), pieces of the union
/// that touch refused, each region extruded from `start` to `end` above `plane`
/// ([`extrude::extrude_region`]) and the solids joined ([`union_all`]).
///
/// # Errors
/// [`Error::Argument`] for no regions; [`Error::Profile`] for a profile the kernel refuses;
/// [`Error::Unsupported`] for pieces that touch, or a sheet not read back exactly;
/// [`Error::Empty`] when no material is left; the kernel's error.
pub fn extrude_planar_regions(
    plane: &Plane,
    regions: &[Region2],
    start: f64,
    end: f64,
) -> Result<Body> {
    if regions.is_empty() {
        return Err(Error::Argument);
    }
    let kr: Vec<_> = regions.iter().map(convert::region2).collect();
    checked(kr.iter(), EXTRUDE_TOLERANCE)?;
    let target = convert::plane(plane);
    let pieces = run("extrude_planar_regions", |k| {
        let union = match sheet_of(k, &kr, EXTRUDE_TOLERANCE)? {
            Ok(s) => s,
            Err(e) => return Ok(Out(Err(e))),
        };
        let read = sheet::sheet_regions(k, &union, &regions_frame(), EXTRUDE_TOLERANCE)?;
        if read.touching {
            return Ok(Out(Err(Error::Unsupported(
                "planar material pieces touch at a boundary",
            ))));
        }
        if read.regions.is_empty() {
            return Ok(Out(Err(Error::Empty)));
        }
        let mut solids = Vec::new();
        for r in &read.regions {
            let b = extrude::extrude_region(k, &target, r, start, end)?;
            match finish(k, b, Expect::SOLID)? {
                Ok(body) => solids.push(body),
                Err(e) => return Ok(Out(Err(e))),
            }
        }
        Ok(Out(Ok(solids)))
    })?;
    union_all(pieces.0?.into_iter().map(Body::new).collect())
}

/// A uniform prism along a plane's normal, as [`prism_section`] proves it.
#[derive(Clone, Debug, PartialEq)]
pub struct PrismSection {
    /// The level of its bottom cap above the plane.
    pub lo: f64,
    /// The level of its top cap (`lo < hi`).
    pub hi: f64,
    /// Its section, exact: lines and arcs, the outer loop counter-clockwise, the holes clockwise.
    pub region: Region2,
    /// How far the levels may be off the body's caps.
    pub tolerance: f64,
    /// An enclosure of its volume.
    pub volume: Interval,
}

impl Answer for PrismSection {}

/// The uniform prism `body` is along `plane`'s normal, when the kernel proves it one on its
/// current geometry ([`measure::section`]): `None` when that is not proved.
///
/// # Errors
/// Those of the session.
pub fn prism_section(body: &Body, plane: &Plane) -> Result<Option<PrismSection>> {
    let f = convert::plane(plane);
    let s = read_body("prism_section", body, |k, b| Ok(measure::section(k, b, &f)))?;
    Ok(s.map(|s| PrismSection {
        lo: s.lo,
        hi: s.hi,
        region: convert::record_region2(&s.region),
        tolerance: s.tolerance,
        volume: s.volume,
    }))
}

/// Whether `a` and `b` are proved the same uniform prism ([`measure::equivalent`]): `None` when
/// that is not proved either way (never a copy granted).
///
/// # Errors
/// Those of the session.
pub fn prism_equivalent(a: &Body, b: &Body) -> Result<Option<bool>> {
    if std::ptr::eq(a, b) {
        return read_body("prism_equivalent", a, |k, g| {
            Ok(measure::equivalent(k, g, g))
        });
    }
    run_on("prism_equivalent", &[("a", a), ("b", b)], |k| {
        let (ga, gb) = (a.lock(), b.lock());
        Ok(measure::equivalent(k, &ga, &gb))
    })
}

/// A free-form curve of several spans in a plane, as the one B-spline it is.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanarSpline {
    /// Its degree.
    pub degree: usize,
    /// Its clamped knots.
    pub knots: Vec<f64>,
    /// Its control points in the plane's coordinates.
    pub points: Vec<Vec2>,
    /// Its weights (empty: all one).
    pub weights: Vec<f64>,
}

/// One piece of an edge read exactly in a plane.
#[derive(Clone, Debug, PartialEq)]
pub enum PlanarPiece {
    /// A line, arc or single free-form segment.
    Seg(Seg2),
    /// A free-form curve of several spans.
    Spline(PlanarSpline),
}

/// Edges read in a plane, with the tolerance they were read to.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanarEdges {
    /// How far a piece read as a line or arc may lie from its edge (mm).
    pub tolerance: Option<f64>,
    /// Each edge's pieces, `None` for one not read exactly in a plane parallel to the plane.
    pub pieces: Vec<Option<Vec<PlanarPiece>>>,
}

impl Answer for PlanarEdges {}

/// Record edges `edges` of `body` in `plane`'s coordinates, exactly ([`read::planar_edges`]):
/// lines, circular arcs, single free-form segments, and a free-form curve of several spans as the
/// one B-spline it is, for a sketch to project them.
///
/// # Errors
/// [`Error::Argument`] for an index past the edges; those of the session.
pub fn planar_edges(
    body: &Body,
    order: &Order,
    plane: &Plane,
    edges: &[usize],
) -> Result<PlanarEdges> {
    let ids = edges
        .iter()
        .map(|i| order::edge(order, *i))
        .collect::<Result<Vec<_>>>()?;
    let f = convert::plane(plane);
    read_body("planar_edges", body, |k, b| {
        let pieces = read::planar_edges(k, b, &f, &ids)
            .into_iter()
            .map(|e| {
                e.map(|ps| {
                    ps.into_iter()
                        .map(|p| match p {
                            read::PlanarPiece::Seg(s) => PlanarPiece::Seg(convert::record_seg2(&s)),
                            read::PlanarPiece::Spline(s) => PlanarPiece::Spline(PlanarSpline {
                                degree: s.degree,
                                knots: s.knots,
                                points: s.points.into_iter().map(convert::record_vec2).collect(),
                                weights: s.weights,
                            }),
                        })
                        .collect()
                })
            })
            .collect();
        Ok(PlanarEdges {
            tolerance: read::planar_edges_tolerance(b),
            pieces,
        })
    })
}

/// The signed area of `region` (its loops' signed areas summed), with a proved bound on its
/// rounding ([`kp::Region2::signed_area`]).
///
/// # Errors
/// [`Error::Profile`] for a malformed segment.
pub fn region_area(region: &Region2) -> Result<kp::Area> {
    convert::region2(region)
        .signed_area()
        .map_err(|_| Error::Profile(ProfileRefusal::FreeForm))
}

/// Whether `lp` holds `p` by the even–odd rule, decided exactly ([`kp::Loop2::holds`]): `None`
/// when `p` is on the loop or within rounding of it.
#[must_use]
pub fn loop_holds(lp: &Loop2, p: Vec2) -> Option<bool> {
    convert::loop2(lp).holds(convert::vec2(p))
}

/// The view `plane`'s coordinates of each of `points`, in their order: one projection for every
/// point a drawing reads on a view ([`kp::to_local_all`]).
///
/// # Errors
/// [`Error::Argument`] for a plane not finite or not orthonormal to within
/// [`kp::VIEW_FRAME_TOL`], more than [`kp::MAX_SEGMENTS`] points, or a point not finite.
pub fn to_local_all(plane: &Plane, points: &[Vec3]) -> Result<Vec<Vec2>> {
    let points: Vec<solidcore::Vec3> = points.iter().map(|q| convert::vec3(*q)).collect();
    let read = kp::to_local_all(&convert::plane(plane), &points).map_err(|_| Error::Argument)?;
    Ok(read.into_iter().map(convert::record_vec2).collect())
}

/// A line on a plane as [`kp::line_coords_2d`] reads it: its unit, its normal (the unit turned a
/// quarter counter-clockwise), and each point as `(along, across)`.
#[derive(Clone, Debug, PartialEq)]
pub struct LineCoords {
    /// The line's unit direction.
    pub unit: Vec2,
    /// `unit` turned a quarter counter-clockwise.
    pub normal: Vec2,
    /// Each point as `(along, across)` the line, in their order.
    pub coords: Vec<Vec2>,
}

/// The line through `origin` along `direction`, and `points` in its coordinates
/// ([`kp::line_coords_2d`]); with no points, the line's unit and normal alone.
///
/// # Errors
/// [`Error::Argument`] for an origin or a point not finite, a direction not finite or of length
/// at or below [`kp::MIN_DIRECTION`], or more than [`kp::MAX_SEGMENTS`] points.
pub fn line_coords_2d(origin: Vec2, direction: Vec2, points: &[Vec2]) -> Result<LineCoords> {
    let points: Vec<solidcore::math::Vec2> = points.iter().map(|q| convert::vec2(*q)).collect();
    let read = kp::line_coords_2d(convert::vec2(origin), convert::vec2(direction), &points)
        .map_err(|_| Error::Argument)?;
    Ok(LineCoords {
        unit: convert::record_vec2(read.unit),
        normal: convert::record_vec2(read.normal),
        coords: read.coords.into_iter().map(convert::record_vec2).collect(),
    })
}
