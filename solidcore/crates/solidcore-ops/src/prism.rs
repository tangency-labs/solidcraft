//! Planar prisms: booleans of planar regions, the extrusion of planar material, whether a body
//! is a uniform prism (its section) or the same prism as another, and edges read in a plane.
//!
//! Each is a recipe of kernel calls (`solidcore::prism`): the kernel checks the profiles, splits
//! their loops, makes their curves, reads sheets back as regions, extrudes a region and proves a
//! prism; ops sequences those with the kernel's planar faces and 2D boolean, the record's
//! planar profiles and planes converted by `solidcore-convert` ([`convert::region2`],
//! [`convert::plane`] and back). It computes no geometric quantity.
//!
//! The booleans keep free-form curves exact: every boundary of a result is a piece of an
//! operand's boundary, cut where it crosses the other, never refitted.
//!
//! [`fill_channel`] is the kernel's channel fill (`solidcore::prism::fill`) in the record's
//! types, its typed refusal with the size that builds returned as a value.

// The range tests refuse NaN by negation (`!(x > y)`), kept as written.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use solidcore::boolean::api_brep::{boolean_2d, Boolean2dOutcome};
use solidcore::error::Reason;
use solidcore::eval::Frame;
use solidcore::kernel::{Kernel, Tolerances};
use solidcore::primitive::api_trim_ops::create_planar_faces;
use solidcore::primitive::primitive_creation::Boolean2d;
use solidcore::prism::{extrude, fill, measure, profile as kp, read, sheet};
use solidcore::Brep;
use solidcore_convert as convert;
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

pub use solidcore::error::fill::{FillFailure, FillSide};
pub use solidcore::prism::fill::{Contour, FillNote, FillOptions};
pub use solidcore::prism::measure::Interval;
pub use solidcore::prism::placement::LoopPlacement;
pub use solidcore::prism::profile::ProfileRefusal;

use crate::body::Contract;
use crate::body::{finish, Body, Expect};
use crate::boolean::{union_all_with, BooleanOp};
use crate::order::{self, Order};
use crate::session::{read_body, run, run_on, Answer};
use crate::{Error, Result};

/// Whether a generated cubic matches a trimmed interval of a source cubic, in either direction.
/// Other segment kinds give false.
#[must_use]
pub fn cubic_contains(source: &Seg2, piece: &Seg2) -> bool {
    convert::seg2(source).cubic_contains(&convert::seg2(piece))
}

/// The tolerance (mm) the planar booleans take: at least this.
pub const MIN_TOLERANCE: f64 = 1e-9;
/// ... and at most this.
pub const MAX_TOLERANCE: f64 = 1.0;
/// The tolerance the planar extrusion closes and splits its loops with (mm).
pub const EXTRUDE_TOLERANCE: f64 = 1e-7;

/// A call's own refusal, else its value, as a kernel call answers inside [`run`].
struct Out<T>(std::result::Result<T, Error>);

impl<T> Answer for Out<T> {
    fn refusal(&self) -> Option<Error> {
        self.0.as_ref().err().cloned()
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

/// `a` op `b` (the kernel's 2D boolean); `None` when nothing is left: no result assigned, or a
/// result with no faces (two disjoint operands intersected).
fn combine(k: &mut Kernel, a: Brep, b: Brep, op: BooleanOp) -> solidcore::Result<Option<Brep>> {
    Ok(match boolean_2d(k, a, b, op2(op))? {
        Boolean2dOutcome::Result(r) if !r.face_list().is_empty() => Some(r),
        Boolean2dOutcome::Result(_) | Boolean2dOutcome::Unassigned { .. } => None,
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

/// The profile check's refusal as the call's, at the zone tolerance of the session's kernel,
/// which builds the profile: the kernel's default, since ops offers no way to change its
/// tolerances ([`crate::session`]), so the check accepts exactly the arcs the build makes.
fn checked<'a>(
    regions: impl Iterator<Item = &'a kp::Region2>,
    tol: f64,
) -> std::result::Result<(), Error> {
    kp::check_regions(regions, tol, Tolerances::default().zone_tol_3d).map_err(Error::Profile)
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
    union_all_with(
        pieces.0?.into_iter().map(Body::new).collect(),
        Contract::Proved,
    )
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
    /// How far the levels and section may be off the body's caps, including frame conversion.
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
        let (ga, gb) = Body::lock_pair(a, b);
        Ok(measure::equivalent(k, &ga, &gb))
    })
}

pub use solidcore_record::projection::{PlanarPiece, PlanarSpline};

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

/// The section loop a closed chain of nodes samples ([`solidcore::prism::fit::fit_loop`]): a
/// full circle where the nodes are one within `tolerance`, else the polygon of their corners,
/// each straight run joined within three times `epsilon`. For a mesh's cap boundary, which the
/// caller certifies against the mesh afterwards.
///
/// # Errors
/// [`Error::Status`] (input refused) past the straight-run join's work bound.
pub fn fitted_loop(points: &[Vec2], tolerance: f64, epsilon: f64) -> Result<Loop2> {
    let points: Vec<_> = points.iter().map(|p| convert::vec2(*p)).collect();
    Ok(
        match solidcore::prism::fit::fit_loop(&points, tolerance, epsilon)
            .map_err(crate::error::status)?
        {
            solidcore::prism::fit::FittedLoop::Circle { center, radius } => {
                Loop2::circle(convert::record_vec2(center), radius)
            }
            solidcore::prism::fit::FittedLoop::Polygon(corners) => Loop2::polygon(
                &corners
                    .into_iter()
                    .map(convert::record_vec2)
                    .collect::<Vec<_>>(),
            ),
        },
    )
}

/// Whether `lp` holds `p` by the even–odd rule, decided exactly ([`kp::Loop2::holds`]): `None`
/// when `p` is on the loop or within rounding of it.
#[must_use]
pub fn loop_holds(lp: &Loop2, p: Vec2) -> Option<bool> {
    convert::loop2(lp).holds(convert::vec2(p))
}

/// Counter-clockwise angle between two planar rays, in radians modulo one turn.
/// Equal rays give zero. Nonfinite and zero rays have no angle.
#[must_use]
pub fn counterclockwise_sweep(center: Vec2, start: Vec2, end: Vec2) -> Option<f64> {
    kp::counterclockwise_sweep(
        convert::vec2(center),
        convert::vec2(start),
        convert::vec2(end),
    )
}

/// The directed parameter interval of a recognized ellipse arc, in the input plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipseArc {
    /// The centre in the input plane.
    pub center: Vec2,
    /// The directed major semiaxis.
    pub major: Vec2,
    /// The positive minor-to-major axis ratio.
    pub ratio: f64,
    /// The starting parameter in radians.
    pub start: f64,
    /// The ending parameter in radians, retaining direction.
    pub end: f64,
}

/// Recognize the supporting ellipse of the given rational quadratic, or refuse.
pub fn conic_ellipse(a: Vec2, apex: Vec2, b: Vec2, w: f64) -> Option<EllipseArc> {
    let arc = solidcore::prism::recognition::conic_ellipse(
        convert::vec2(a),
        convert::vec2(apex),
        convert::vec2(b),
        w,
    )?;
    Some(EllipseArc {
        center: convert::record_vec2(arc.center),
        major: convert::record_vec2(arc.major),
        ratio: arc.ratio,
        start: arc.start,
        end: arc.end,
    })
}

/// Whether a piece of a planar curve within the reading tolerance of its chord is read as the
/// arc it is proved on before that line ([`read::arc_before_line`]): `spans` its Bézier spans
/// (`[x w, y w, w]` in the plane), `zone` the model's zone tolerance
/// ([`crate::sweep::zone_tolerance`]).
#[must_use]
pub fn arc_before_line(spans: &[Vec<[f64; 3]>], zone: f64) -> bool {
    read::arc_before_line(spans, zone)
}

/// Recognize one directed turn of arcs with the same stored support.
pub fn loop_circle(lp: &Loop2) -> Option<(Vec2, f64)> {
    if lp.segs.len() > 200_000 {
        return None;
    }
    let (center, radius) = solidcore::prism::recognition::loop_circle(&convert::loop2(lp))?;
    Some((convert::record_vec2(center), radius))
}

/// Recognize one ellipse turn without changing any input control point.
pub fn loop_ellipse(lp: &Loop2) -> Option<(Vec2, Vec2, f64)> {
    if lp.segs.len() > 200_000 {
        return None;
    }
    let (center, major, ratio) = solidcore::prism::recognition::loop_ellipse(&convert::loop2(lp))?;
    Some((
        convert::record_vec2(center),
        convert::record_vec2(major),
        ratio,
    ))
}

/// Read the signed single winding of ordered points already admitted on one round wall.
pub fn certified_round_wall_turn(points: &[Vec2], center: Vec2) -> Option<f64> {
    if points.len() > 200_000 {
        return None;
    }
    solidcore::prism::recognition::certified_round_wall_turn(
        &points
            .iter()
            .copied()
            .map(convert::vec2)
            .collect::<Vec<_>>(),
        convert::vec2(center),
    )
}

/// Where `lp` lies against `other` (both in one plane's coordinates), loops within `tol` of each
/// other counting as meeting ([`kp::Loop2::placement`]): clear inside,
/// clear outside, along it everywhere, or meeting it in places only. `None` for loops of other
/// than lines and arcs, or a placement the kernel cannot decide.
#[must_use]
pub fn loop_placement(lp: &Loop2, other: &Loop2, tol: f64) -> Option<LoopPlacement> {
    convert::loop2(lp).placement(&convert::loop2(other), tol)
}

/// Positive and negative wing `(start, end)` intervals for a symmetric-offset extrude.
#[must_use]
pub fn symmetric_offset_intervals(gap: f64, half: f64) -> [(f64, f64); 2] {
    extrude::symmetric_offset_intervals(gap, half)
}

/// Probe points along both wings of a symmetric-offset extrude for auto-operation detection.
#[must_use]
pub fn symmetric_offset_probe_points(
    base: Vec3,
    n: Vec3,
    start: f64,
    d: f64,
    near_limit: f64,
) -> [Vec3; 4] {
    extrude::symmetric_offset_probe_points(
        [base.x, base.y, base.z],
        [n.x, n.y, n.z],
        start,
        d,
        near_limit,
    )
    .map(|[x, y, z]| Vec3::new(x, y, z))
}

/// Where the line along `sketch`'s normal through `point` meets `plane`, as a level along that
/// normal from `sketch`'s origin ([`extrude::level_on_plane`]): an extrude's end on a flat face
/// at a slant to it.
///
/// # Errors
///
/// [`Error::Refused`] for a plane along the normal or a level that is not finite.
pub fn level_on_plane(sketch: &Plane, point: Vec3, plane: &Plane) -> Result<f64> {
    extrude::level_on_plane(
        &convert::plane(sketch),
        convert::vec3(point),
        &convert::plane(plane),
    )
    .map_err(crate::error::status)
}

/// The least and the most level of `plane` above `sketch` over the outer loops of `regions`
/// ([`extrude::plane_rise`]): how far a flat face at a slant to an extrude rises across its
/// profiles.
///
/// # Errors
///
/// [`Error::Refused`] for a plane along the normal, no outer segment or a malformed segment;
/// [`Error::Status`] for a free-form piece whose search does not settle.
pub fn plane_rise(sketch: &Plane, plane: &Plane, regions: &[Region2]) -> Result<(f64, f64)> {
    let regions: Vec<_> = regions.iter().map(convert::region2).collect();
    extrude::plane_rise(&convert::plane(sketch), &convert::plane(plane), &regions)
        .map_err(crate::error::status)
}

/// The least and the most of `to`'s level above `sketch` less `from`'s over the outer loops of
/// `regions` ([`extrude::plane_gap`]): the gap an extrude From one flat face To another runs
/// across its profiles.
///
/// # Errors
///
/// As [`plane_rise`].
pub fn plane_gap(
    sketch: &Plane,
    from: &Plane,
    to: &Plane,
    regions: &[Region2],
) -> Result<(f64, f64)> {
    let regions: Vec<_> = regions.iter().map(convert::region2).collect();
    extrude::plane_gap(
        &convert::plane(sketch),
        &convert::plane(from),
        &convert::plane(to),
        &regions,
    )
    .map_err(crate::error::status)
}

/// A channel filled by [`fill_channel`] ([`fill::Filled`] in the record's types).
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelFill {
    /// The filled section: one counter-clockwise loop, no holes.
    pub region: Region2,
    /// How deep the channel reaches, measured as the depth is.
    pub channel_depth: f64,
    /// How deep it is filled.
    pub filled_depth: f64,
    /// Whether the channel is open on the far side.
    pub through: bool,
    /// What a caller may report.
    pub notes: Vec<FillNote>,
}

/// Why [`fill_channel`] refuses a fill, and the size that builds where there is one (the
/// kernel's [`FillFailure`] and its fit).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FillRefusal {
    /// Why.
    pub why: FillFailure,
    /// The nearest size that builds (a face's height or depth), where there is one.
    pub fit: Option<f64>,
}

/// [`fill_channel`]'s answer inside [`run`]: a fill or its refusal, neither an error.
struct Filling(std::result::Result<ChannelFill, FillRefusal>);

impl Answer for Filling {}

/// The channel between the walls `left` and `right` filled with `options`
/// ([`fill::fill`]): the depth measured along `depth_normal` from the deeper toe where it is
/// given, else from the chord. A refusal the kernel types ([`FillFailure`]) comes back as
/// `Ok(Err(..))` with its fit.
///
/// # Errors
/// [`Error::Refused`] for walls, a normal, contours or a depth the kernel refuses as input;
/// the kernel's other errors.
pub fn fill_channel(
    left: &[Seg2],
    right: &[Seg2],
    depth_normal: Option<Vec2>,
    options: &FillOptions,
) -> Result<std::result::Result<ChannelFill, FillRefusal>> {
    let channel = fill::Channel {
        left: left.iter().map(convert::seg2).collect(),
        right: right.iter().map(convert::seg2).collect(),
        depth_normal: depth_normal.map(convert::vec2),
    };
    let out = run("fill_channel", |k| match fill::fill(k, &channel, options) {
        Ok(f) => Ok(Filling(Ok(ChannelFill {
            region: convert::record_region2(&f.region),
            channel_depth: f.channel_depth,
            filled_depth: f.filled_depth,
            through: f.through,
            notes: f.notes,
        }))),
        Err(e) => match e.failure().map(|f| &f.reason) {
            Some(&Reason::Fill(why)) => Ok(Filling(Err(FillRefusal {
                why,
                fit: e.fit().map(|f| f.size),
            }))),
            _ => Err(e),
        },
    })?;
    Ok(out.0)
}
