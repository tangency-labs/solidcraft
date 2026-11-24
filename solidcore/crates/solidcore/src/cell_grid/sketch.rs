//! A sketched cell: one closed profile of lines and arcs, repeated over the field on a lattice
//! given by pitch, stagger, rotation and flip.
//!
//! # The cell
//! The profile is a [`Loop2`] in its own coordinates. It is refused when it is open (an end is
//! more than 0.0001 mm from the next start), crosses or touches itself, has no area, holds a spline
//! or conic, or reaches more than [`MAX_CELL_REACH`] mm from its origin. Its origin is the point
//! given, or else its centroid (the exact centroid of the region it bounds, an arc's segment
//! included). The cell is placed by taking the origin to the lattice point, mirroring it in the
//! vertical axis through its origin when it is a flipped cell, and turning it by `cell_angle` about
//! its origin.
//!
//! # The lattice
//! `pitch_x`, `pitch_y` and `stagger` are turned into the two lattice vectors in one place
//! ([`lattice_vectors`]): `a = (pitch_x, 0)` and `b = (stagger * pitch_x, pitch_y)`. Cell `(i, j)`
//! has its origin at `i * a + j * b`, always worked out from the integer indices, so two fields with
//! the same parameters line up. Each row is `pitch_y` above the last and shifted along the row by
//! `stagger * pitch_x` against it: at `0.5` the rows alternate half a pitch (hexagonal and brick
//! layouts), at `0` they stack, and at other values the lattice is skewed (row `j` is `j * stagger *
//! pitch_x` along). `angle` turns the whole grid about `anchor`, and `anchor` is where the origin of
//! cell `(0, 0)` sits, as for the built-in cells. With `flip`, the cells with `i + j` odd (alternate
//! cells in a row and alternate rows) are mirrored; with no flip all cells are the same.
//!
//! # The clearance
//! `web` is the least distance between two neighbouring cells' outlines, checked exactly on the
//! lines and arcs ([`gap`]), and it is checked before anything is planned. Every cell lies in the
//! disc of radius `R` (the reach) about its origin, turning and mirroring about the origin keep
//! it, so two cells whose origins are more than `2 R + web` apart are more than `web` apart and
//! need no test. Because the cells are translates of one another (two kinds with `flip`), testing
//! a cell against every lattice offset within `2 R + web` tests every pair that can be nearer than
//! the web. More than [`MAX_OFFSETS`] such offsets is refused (the pitch is small for the cell). A
//! cell that overlaps a neighbour (its interior meets the neighbour's, by an exact clip of one
//! against the other, so cells that share an edge exactly do not overlap) is refused, as is one
//! nearer than the web.
//!
//! # Clipping
//! [`Clip::Whole`] keeps the cells entirely inside the field, clear of every keep-out and, with a
//! margin, that far from every edge. [`Clip::Free`] trims the cells that cross an edge to the field
//! exactly (the same trimming as the built-in cells, which needs no convexity). [`Clip::Snap`] is
//! refused: it moves field edges onto the cell's own chords, which a sketched cell does not have.
//! The nearness of a spline edge is measured as for the built-in cells (see [`super::exact`]).

use crate::math::Vec2;
use crate::prism::profile::{Loop2, Seg2};

use super::exact::ExactField;
use super::free::{self, Cell as FreeCell};
use super::gap;
use super::region::{poly_area, poly_centroid, rotate, Bounds, Loop, Prim, Region, EPS};
use super::vec::Vec2Ext;
use super::{
    check_seg, lattice_loops, push, CellGridError, CellGridPlan, CellGridSpec, CellKind, Clip,
    MAX_CELLS, MAX_COORD, MIN_SIZE, OPEN_TOL,
};

/// Most segments in a cell's profile.
pub const MAX_CELL_SEGMENTS: usize = 256;
/// Farthest (mm) any point of a cell may be from its origin.
pub const MAX_CELL_REACH: f64 = 1e6;
/// Most lattice offsets a cell is tested against.
const MAX_OFFSETS: usize = 2_000;
/// Smallest area (mm²) of a cell.
const MIN_CELL_AREA: f64 = MIN_SIZE * MIN_SIZE;
/// How far inside the web two cells may come and still count as the web apart: a rounding.
const GRAZE: f64 = 1e-9;
/// Two outlines this near meet (their interiors are then tested for overlap).
const TOUCH: f64 = 1e-6;

/// A sketched cell and how it is laid out, in the sketch's units (mm and radians).
#[derive(Clone, Debug, PartialEq)]
pub struct SketchCell {
    /// The cell's outline, a closed loop of lines and arcs in the cell's own coordinates.
    pub profile: Loop2,
    /// The cell's origin in those coordinates (`None`: the centroid of the outline).
    pub origin: Option<Vec2>,
    /// The distance along a row between cell origins, from [`MIN_SIZE`] to [`MAX_COORD`].
    pub pitch_x: f64,
    /// The distance between rows, from [`MIN_SIZE`] to [`MAX_COORD`].
    pub pitch_y: f64,
    /// How far each row is shifted along the row against the one before, as a fraction of
    /// `pitch_x`, from 0 to 1 (0.5: alternate rows half a pitch apart).
    pub stagger: f64,
    /// The cell's turn about its own origin (radians), separate from the grid's `angle`.
    pub cell_angle: f64,
    /// Mirror the cells with `i + j` odd in the vertical axis through their origins.
    pub flip: bool,
}

/// The lattice vectors `(a, b)` of the layout: the one place `pitch_x`, `pitch_y` and `stagger`
/// become vectors.
fn lattice_vectors(pitch_x: f64, pitch_y: f64, stagger: f64) -> (Vec2, Vec2) {
    (
        Vec2::new(pitch_x, 0.0),
        Vec2::new(stagger * pitch_x, pitch_y),
    )
}

/// The layout's numbers checked, as the lattice vectors.
fn layout(cell: &SketchCell) -> Result<(Vec2, Vec2), CellGridError> {
    let pitch = |what: &str, v: f64| {
        if v.is_finite() && (MIN_SIZE..=MAX_COORD).contains(&v) {
            Ok(())
        } else {
            Err(CellGridError::Profile(format!(
                "{what} must be from {MIN_SIZE} to {MAX_COORD} mm, not {v}"
            )))
        }
    };
    pitch("pitch_x", cell.pitch_x)?;
    pitch("pitch_y", cell.pitch_y)?;
    if !(cell.stagger.is_finite() && (0.0..=1.0).contains(&cell.stagger)) {
        return Err(CellGridError::Profile(format!(
            "the stagger must be from 0 to 1, not {}",
            cell.stagger
        )));
    }
    if !(cell.cell_angle.is_finite() && cell.cell_angle.abs() <= 1e6) {
        return Err(CellGridError::Profile(format!(
            "the cell angle must be finite and within a million radians, not {}",
            cell.cell_angle
        )));
    }
    if let Some(o) = cell.origin {
        if !(o.is_finite() && o.x.abs() <= MAX_COORD && o.y.abs() <= MAX_COORD) {
            return Err(CellGridError::Profile(
                "the origin must be a finite point within a billion mm of the profile's zero"
                    .into(),
            ));
        }
    }
    Ok(lattice_vectors(cell.pitch_x, cell.pitch_y, cell.stagger))
}

/// The profile's pieces, refused unless it is a closed loop of lines and arcs.
fn profile_prims(profile: &Loop2) -> Result<Vec<Prim>, CellGridError> {
    let bad = CellGridError::Profile;
    if profile.segs.is_empty() {
        return Err(bad("has no segments".into()));
    }
    if profile.segs.len() > MAX_CELL_SEGMENTS {
        return Err(bad(format!(
            "has {} segments, more than the {MAX_CELL_SEGMENTS} allowed",
            profile.segs.len()
        )));
    }
    let mut prims = Vec::with_capacity(profile.segs.len());
    for (k, seg) in profile.segs.iter().enumerate() {
        check_seg(seg, MAX_COORD).map_err(|what| bad(format!("segment {k}: {what}")))?;
        match *seg {
            Seg2::Line { a, b } => prims.push(Prim::line(a, b)),
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => prims.push(Prim::arc(center, radius, start, sweep)),
            _ => {
                return Err(bad(format!(
                    "segment {k} is a spline or conic: a cell is made of lines and arcs"
                )))
            }
        }
    }
    if !profile.is_closed(OPEN_TOL) {
        return Err(bad(format!(
            "is not closed: a segment ends more than {OPEN_TOL} mm from where the next starts"
        )));
    }
    Ok(prims)
}

/// A pair of pieces of the loop that cross or touch other than at the joint they share.
fn self_intersection(prims: &[Prim]) -> Option<(usize, usize)> {
    let n = prims.len();
    for i in 0..n {
        for j in (i + 1)..n {
            let (Some(p), Some(q)) = (prims.get(i), prims.get(j)) else {
                continue;
            };
            if !p.bounds().overlaps(&q.bounds(), 1e-9) {
                continue;
            }
            let hits = free::intersections(p, q);
            if hits.is_empty() {
                continue;
            }
            if j != i + 1 && !(i == 0 && j + 1 == n) {
                return Some((i, j));
            }
            // Neighbours meet at their joint (or two, for a loop of two pieces) and nowhere else.
            let joints: Vec<Vec2> = [(p.end(), q.start()), (q.end(), p.start())]
                .into_iter()
                .filter(|(x, y)| x.distance(*y) <= TOUCH)
                .map(|(x, _)| x)
                .collect();
            if hits
                .iter()
                .any(|h| !joints.iter().any(|jt| jt.distance(*h) <= TOUCH))
            {
                return Some((i, j));
            }
        }
    }
    None
}

/// The centroid of the region a loop of lines and arcs bounds: the polygon on the pieces' start
/// points, and each arc's circular segment (signed by its sweep, so a concave arc takes area
/// away) with its centroid on the bisector at `4 r sin^3(h) / (3 (2 h - sin 2 h))` from the
/// centre, `h` half the sweep. A segment under 1e-4 radians (an area below `r^2 * 1e-13`) is left out.
fn centroid(prims: &[Prim]) -> Vec2 {
    let pts: Vec<Vec2> = prims.iter().map(Prim::start).collect();
    let mut area = poly_area(&pts);
    let mut moment = poly_centroid(&pts) * area;
    for p in prims {
        if let Prim::Arc {
            c, r, a0, sweep, ..
        } = *p
        {
            let theta = sweep.abs();
            if theta < 1e-4 {
                continue;
            }
            let s = 0.5 * r * r * (sweep - sweep.sin());
            let dist = 4.0 * r * (theta / 2.0).sin().powi(3) / (3.0 * (theta - theta.sin()));
            let mid = a0 + sweep / 2.0;
            area += s;
            moment += (c + Vec2::new(mid.cos(), mid.sin()) * dist) * s;
        }
    }
    if area.abs() > 1e-300 {
        moment / area
    } else {
        poly_centroid(&pts)
    }
}

/// The farthest any point of the pieces is from the origin: an end, or the point of an arc's
/// circle straight out from the origin through its centre, if the arc passes it.
fn reach_of(prims: &[Prim]) -> f64 {
    let mut m = 0.0_f64;
    for p in prims {
        m = m.max(p.start().length()).max(p.end().length());
        if let Prim::Arc { c, r, .. } = *p {
            if p.contains_angle(c.y.atan2(c.x), 0.0) {
                m = m.max(c.length() + r);
            }
        }
    }
    m
}

/// A piece mirrored in the vertical axis through the origin.
fn mirror(p: &Prim) -> Prim {
    match *p {
        Prim::Line { a, b, lattice } => Prim::Line {
            a: Vec2::new(-a.x, a.y),
            b: Vec2::new(-b.x, b.y),
            lattice,
        },
        Prim::Arc {
            c, r, a0, sweep, ..
        } => Prim::arc(Vec2::new(-c.x, c.y), r, std::f64::consts::PI - a0, -sweep),
    }
}

/// The cell at the lattice origin: moved so its origin is at 0, mirrored when `flip`, turned by
/// `angle`, counter-clockwise (mirroring reverses a loop). `None` for a coordinate that is not
/// finite.
fn place(prims: &[Prim], origin: Vec2, flip: bool, angle: f64) -> Option<Loop> {
    let placed: Vec<Prim> = prims
        .iter()
        .map(|p| {
            let q = p.moved(0.0, -origin);
            let q = if flip { mirror(&q) } else { q };
            q.moved(angle, Vec2::ZERO)
        })
        .collect();
    Some(Loop::new(placed, None)?.oriented(true))
}

/// A checked cell and layout.
struct Prepared {
    /// The cell at the lattice origin, counter-clockwise: the ordinary one and the mirrored one
    /// (the same when there is no flip).
    loops: [Loop; 2],
    flip: bool,
    a: Vec2,
    b: Vec2,
    /// Farthest any point of the cell is from its origin.
    reach: f64,
    area: f64,
    web: f64,
}

/// What two neighbouring outlines that meet or nest do.
enum Meet {
    Apart,
    Overlap,
    Unclear,
}

/// Whether the cell `p` and the loop of pieces `q` overlap (their interiors meet), given the
/// least distance `dist` between their outlines.
fn meet(p: &Loop, q: &Loop, dist: f64, area: f64, reach: f64) -> Meet {
    if dist > TOUCH {
        // The outlines are apart, so one lies wholly inside the other or they are separate.
        let inside = |lp: &Loop, other: &Loop| {
            other
                .prims
                .first()
                .is_some_and(|first| lp.contains(first.start()))
        };
        return if inside(p, q) || inside(q, p) {
            Meet::Overlap
        } else {
            Meet::Apart
        };
    }
    // They touch or cross: the part of the cell inside the neighbour, exactly. Where the two run
    // along each other the clip keeps a piece only when both run the same way, so cells that share
    // an edge have no common area.
    let region = Region::from_loops(vec![q.clone()], reach.max(1e-9));
    match free::clip_cell(&region, &FreeCell::General(p)) {
        None => Meet::Unclear,
        Some(pieces) => {
            let common: f64 = pieces.iter().map(|x| x.1).sum();
            if common > 1e-9 * area {
                Meet::Overlap
            } else {
                Meet::Apart
            }
        }
    }
}

/// The lattice offsets `(di, dj)` other than `(0, 0)` with `|di * a + dj * b|` possibly at most
/// `l`: every `dj` with `|dj| * b.y <= l`, and for each the `di` with `|x|` at most `l` along the row.
fn offsets(a: Vec2, b: Vec2, l: f64) -> Result<Vec<(i64, i64)>, CellGridError> {
    let too_many = || {
        CellGridError::Layout(format!(
            "the pitch is small for the cell: more than {MAX_OFFSETS} neighbouring cells lie \
             within {l:.1} mm of a cell; use a larger pitch_x or pitch_y, or a smaller cell"
        ))
    };
    let jmax = (l / b.y).floor();
    if !(jmax <= MAX_OFFSETS as f64) {
        return Err(too_many());
    }
    let jm = jmax as i64;
    let mut out: Vec<(i64, i64)> = Vec::new();
    for dj in -jm..=jm {
        let shift = dj as f64 * b.x;
        let lo = ((-l - shift) / a.x).ceil();
        let hi = ((l - shift) / a.x).floor();
        let n = hi - lo + 1.0;
        if !(n >= 1.0) {
            continue;
        }
        if !(out.len() as f64 + n <= MAX_OFFSETS as f64) {
            return Err(too_many());
        }
        for di in (lo as i64..).take(n as usize) {
            if !(di == 0 && dj == 0) {
                out.push((di, dj));
            }
        }
    }
    Ok(out)
}

/// Refuses a layout in which a cell overlaps a neighbour or comes nearer than the web.
fn check_neighbours(p: &Prepared) -> Result<(), CellGridError> {
    let offs = offsets(p.a, p.b, 2.0 * p.reach + p.web)?;
    let kinds: &[usize] = if p.flip { &[0, 1] } else { &[0] };
    for &kind in kinds {
        let Some(base) = p.loops.get(kind) else {
            continue;
        };
        let bb = base.bounds();
        for &(di, dj) in &offs {
            // The neighbour is mirrored against the cell when `i + j` changes parity.
            let other_kind = if p.flip && ((di ^ dj) & 1) == 1 {
                1 - kind
            } else {
                kind
            };
            let Some(other) = p.loops.get(other_kind) else {
                continue;
            };
            let d = p.a * (di as f64) + p.b * (dj as f64);
            let ob = other.bounds();
            let shifted = Bounds {
                x0: ob.x0 + d.x,
                y0: ob.y0 + d.y,
                x1: ob.x1 + d.x,
                y1: ob.y1 + d.y,
            };
            if !bb.overlaps(&shifted, p.web) {
                continue;
            }
            let moved: Vec<Prim> = other.prims.iter().map(|q| q.moved(0.0, d)).collect();
            let dist = gap::boundary_distance(&base.prims, &moved);
            let Some(neighbour) = Loop::new(moved, None) else {
                continue;
            };
            match meet(base, &neighbour, dist, p.area, p.reach) {
                Meet::Overlap => {
                    return Err(CellGridError::Layout(format!(
                        "a cell overlaps its neighbour at lattice offset ({di}, {dj}): increase \
                         pitch_x or pitch_y, change the stagger, or shrink or turn the cell"
                    )))
                }
                Meet::Unclear => {
                    return Err(CellGridError::Layout(format!(
                        "a cell touches its neighbour at lattice offset ({di}, {dj}) in a way \
                         that cannot be told from an overlap: change the pitch or the web"
                    )))
                }
                Meet::Apart => {}
            }
            if dist < p.web - GRAZE {
                return Err(CellGridError::Layout(format!(
                    "a cell comes within {dist:.6} mm of its neighbour at lattice offset ({di}, \
                     {dj}), closer than the web of {} mm: increase pitch_x or pitch_y, or reduce \
                     the web",
                    p.web
                )));
            }
        }
    }
    Ok(())
}

fn prepare(cell: &SketchCell, spec: &CellGridSpec) -> Result<Prepared, CellGridError> {
    if spec.clip == Clip::Snap || spec.keepout_clip == Some(Clip::Snap) {
        return Err(CellGridError::Parameter(
            "snap needs the cell's own chords; use whole or free".into(),
        ));
    }
    if !(spec.web.is_finite() && (0.0..=MAX_COORD).contains(&spec.web)) {
        return Err(CellGridError::Parameter(format!(
            "the web must be from 0 to {MAX_COORD} mm, not {}",
            spec.web
        )));
    }
    spec.validate_placement()?;
    let (a, b) = layout(cell)?;
    let prims = profile_prims(&cell.profile)?;
    if let Some((i, j)) = self_intersection(&prims) {
        return Err(CellGridError::Profile(format!(
            "segments {i} and {j} cross or touch: a cell may not cross itself"
        )));
    }
    let origin = cell.origin.unwrap_or_else(|| centroid(&prims));
    let finite = || CellGridError::Profile("has a coordinate that is not finite".into());
    if !origin.is_finite() {
        return Err(finite());
    }
    let even = place(&prims, origin, false, cell.cell_angle).ok_or_else(finite)?;
    let area = even.signed_area();
    if !(area.is_finite() && area >= MIN_CELL_AREA) {
        return Err(CellGridError::Profile(format!(
            "has no area: it must enclose at least {MIN_CELL_AREA} mm2, not {area}"
        )));
    }
    let reach = reach_of(&even.prims);
    if !(reach.is_finite() && reach <= MAX_CELL_REACH) {
        return Err(CellGridError::Profile(format!(
            "reaches {reach} mm from its origin, more than the {MAX_CELL_REACH} mm allowed"
        )));
    }
    let odd = if cell.flip {
        place(&prims, origin, true, cell.cell_angle).ok_or_else(finite)?
    } else {
        even.clone()
    };
    // Cells all of one area cannot fit a lattice whose cell is smaller.
    let lattice_area = a.x * b.y;
    if area > lattice_area * (1.0 + 1e-9) {
        return Err(CellGridError::Layout(format!(
            "the cell's area, {area:.6} mm2, is larger than the {lattice_area:.6} mm2 each cell \
             has (pitch_x * pitch_y): the cells would overlap; increase the pitch"
        )));
    }
    let prepared = Prepared {
        loops: [even, odd],
        flip: cell.flip,
        a,
        b,
        reach,
        area,
        web: spec.web,
    };
    check_neighbours(&prepared)?;
    Ok(prepared)
}

/// The centre, in the sketch plane, of lattice cell `(i, j)` (the cell's origin), for a sketched
/// cell: `anchor + rotate(angle) * (i * a + j * b)`.
///
/// # Errors
///
/// [`CellGridError`] when a parameter is refused.
pub fn sketch_cell_center(
    spec: &CellGridSpec,
    cell: &SketchCell,
    i: i64,
    j: i64,
) -> Result<Vec2, CellGridError> {
    spec.validate_placement()?;
    let (a, b) = layout(cell)?;
    Ok(spec.anchor + rotate(a * (i as f64) + b * (j as f64), spec.angle))
}

/// The lattice cells `(i, j)` whose origin is within `reach` of `b`, or an error when there are
/// more than [`MAX_CELLS`] (counted in floats first, so nothing overflows).
fn cells_in(prep: &Prepared, b: &Bounds) -> Result<Vec<(i64, i64)>, CellGridError> {
    let too_many = |n: f64| CellGridError::TooManyCells {
        cells: if n.is_finite() && n < u64::MAX as f64 {
            n as u64
        } else {
            u64::MAX
        },
        max: MAX_CELLS,
    };
    let r = prep.reach;
    let j0 = ((b.y0 - r) / prep.b.y).ceil();
    let j1 = ((b.y1 + r) / prep.b.y).floor();
    let rows = j1 - j0 + 1.0;
    if !(rows >= 1.0) {
        return Ok(Vec::new());
    }
    if !(rows <= MAX_CELLS as f64 * 4.0) {
        return Err(too_many(rows));
    }
    let mut out: Vec<(i64, i64)> = Vec::new();
    let mut j = j0;
    while j <= j1 {
        let shift = j * prep.b.x;
        let i0 = ((b.x0 - r - shift) / prep.a.x).ceil();
        let i1 = ((b.x1 + r - shift) / prep.a.x).floor();
        let n = i1 - i0 + 1.0;
        if n >= 1.0 {
            if !(out.len() as f64 + n <= MAX_CELLS as f64) {
                return Err(too_many(out.len() as f64 + n));
            }
            let mut i = i0;
            while i <= i1 {
                out.push((i as i64, j as i64));
                i += 1.0;
            }
        }
        j += 1.0;
    }
    Ok(out)
}

/// Which kinds of boundary a cell's trimmed pieces were cut by: `(a field edge, a keep-out edge)`.
/// A piece of the result that does not run along the cell lies on the field's boundary, and the
/// boundary piece it lies on says which kind. A cut that is only along the cell's own pieces counts
/// as a field edge.
fn hit_kinds(region: &Region, cell: &[Prim], pieces: &[(Vec<Prim>, f64)]) -> (bool, bool) {
    let (mut field, mut keepout) = (false, false);
    for (lp, _) in pieces {
        for sub in lp {
            let mid = sub.mid();
            if cell.iter().any(|c| c.distance(mid) <= 1e-7) {
                continue;
            }
            let probe = Bounds {
                x0: mid.x,
                y0: mid.y,
                x1: mid.x,
                y1: mid.y,
            }
            .grown(1e-6);
            let best = region
                .near(&probe)
                .into_iter()
                .filter_map(|k| region.prims.get(k).map(|p| (k, p.distance(mid))))
                .filter(|&(_, d)| d <= 1e-6)
                .min_by(|x, y| x.1.total_cmp(&y.1));
            if let Some((k, _)) = best {
                if region.keepout.get(k).copied().unwrap_or(false) {
                    keepout = true;
                } else {
                    field = true;
                }
            }
        }
    }
    (field || !keepout, keepout)
}

/// The cells of a sketched cell on a lattice over a field given as closed loops (see the
/// module docs and [`super`]). `spec` gives `web`, `angle`, `anchor`, `clip` (`Whole` or `Free`;
/// `Snap` is refused), `keepout_clip` and `margin`; its shape, size, sides, dot grid and snap
/// inset play no part.
///
/// # Errors
///
/// [`CellGridError::Parameter`] for a refused parameter or `Clip::Snap`,
/// [`CellGridError::Profile`] for a profile or layout number that is refused,
/// [`CellGridError::Layout`] when a cell overlaps a neighbour or comes nearer than the web,
/// [`CellGridError::Field`] for a field that is refused, [`CellGridError::TooManyCells`] when the
/// field reaches more than [`MAX_CELLS`] lattice cells.
pub fn plan_sketch(
    field: &[Loop2],
    cell: &SketchCell,
    spec: &CellGridSpec,
) -> Result<CellGridPlan, CellGridError> {
    let prep = prepare(cell, spec)?;
    let mut warnings = Vec::new();
    let (loops, edges, flattened) = lattice_loops(field, spec, &mut warnings)?;
    let pitch = prep.a.x.max(prep.b.y);
    let region = Region::from_loops(loops, pitch);
    let field_clip = spec.clip;
    let keepout_clip = spec.keepout_clip.unwrap_or(spec.clip);
    // A cell clear of every edge may still be dropped for being near a curve or inside a margin,
    // but only when every edge's mode is `Whole`.
    let strict = field_clip == Clip::Whole && keepout_clip == Clip::Whole;
    let exact = (flattened || spec.margin > 0.0).then(|| ExactField::new(edges, pitch));
    let mut out = CellGridPlan {
        warnings,
        ..CellGridPlan::default()
    };
    let Some(bounds) = region.bounds() else {
        return Ok(out);
    };
    for (i, j) in cells_in(&prep, &bounds)? {
        let c = prep.a * (i as f64) + prep.b * (j as f64);
        let kind = usize::from(prep.flip && ((i ^ j) & 1) == 1);
        let Some(base) = prep.loops.get(kind) else {
            continue;
        };
        let prims: Vec<Prim> = base.prims.iter().map(|p| p.moved(0.0, c)).collect();
        let Some(cl) = Loop::new(prims, None) else {
            continue;
        };
        let near = region.near(&cl.bounds().grown(EPS));
        let mut pieces: Vec<(Vec<Prim>, f64)> = Vec::new();
        let full = if near.is_empty() {
            // No edge in reach: the cell is wholly inside the field or wholly outside it.
            let Some(first) = cl.prims.first() else {
                continue;
            };
            if !region.contains(first.start()) {
                continue;
            }
            true
        } else {
            if let Some(p) = free::clip_cell(&region, &FreeCell::General(&cl)) {
                pieces = p;
            } else {
                out.warnings.push(format!(
                    "cell ({i}, {j}) could not be trimmed to the field and was left solid"
                ));
                continue;
            }
            pieces.len() == 1
                && pieces
                    .first()
                    .is_some_and(|p| (p.1 - prep.area).abs() <= 1e-9 * prep.area)
        };
        if full {
            if strict {
                if let Some(ex) = &exact {
                    let margin = spec.margin;
                    let limit = if margin > 0.0 { margin - GRAZE } else { 0.0 };
                    if ex.within_prims(&cl.prims, &cl.bounds(), limit, margin <= 0.0) {
                        continue;
                    }
                }
            }
            push(
                &mut out,
                spec,
                (i, j, c),
                CellKind::Whole,
                vec![cl.prims.clone()],
                prep.area,
            );
            continue;
        }
        if pieces.is_empty() {
            continue;
        }
        let (on_field, on_keepout) = hit_kinds(&region, &cl.prims, &pieces);
        // A cell is clipped by the mode of the edges it meets; one that meets both kinds gets the
        // stricter: dropped for whole, else trimmed.
        let mode = match (on_field, on_keepout) {
            (false, true) => keepout_clip,
            (_, false) => field_clip,
            (true, true) => {
                if field_clip == Clip::Whole || keepout_clip == Clip::Whole {
                    Clip::Whole
                } else {
                    Clip::Free
                }
            }
        };
        if mode != Clip::Free {
            continue;
        }
        let area: f64 = pieces.iter().map(|p| p.1).sum();
        if area > 1e-9 {
            push(
                &mut out,
                spec,
                (i, j, c),
                CellKind::Free,
                pieces.into_iter().map(|p| p.0).collect(),
                area,
            );
        }
    }
    Ok(out)
}
