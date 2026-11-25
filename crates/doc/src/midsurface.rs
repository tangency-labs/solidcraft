//! Midsurface: the sheets halfway between the faces of a thin-walled solid that face each other
//! across material, for a shell model (FEA). Two steps: [`analyze`] is read-only and proposes
//! pairs of faces with the evidence for each (thickness, overlap, aspect, confidence, notes) and
//! says why every other face is in no pair; a caller (a person, an agent, or [`confident`]) then
//! chooses the pairs, and [`sheets`] and [`surface_body`] build them.
//!
//! Flat walls (two planes) and round walls (two coaxial cylinders) pair. Every decision is made on
//! the exact faces: the overlap is an exact region boolean, "is anything in between?" an exact
//! ray, a rim's width the exact points of its edges against the wall (solidcore's
//! `mid_sheet::wall_holds`). The geometry itself (facing faces, joints, areas, projections) is
//! solidcore's (`solidcraft_kernel::services::{face_pair, mid_joint, mid_sheet}`); this module chooses and
//! sequences.

use solidcore_record::brep::Loop as Bounds;
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::services::face_pair::{
    CylinderFace, OutwardPlane, axial_range_in, facing_cylinders, facing_planes, material_toward_axis, round_directions,
};
use solidcraft_kernel::services::mid_joint::{move_edge_onto_line, planes_meeting};
use solidcraft_kernel::services::mid_sheet::{self as ms, Frame, Wall};
use solidcraft_kernel::{Body, BoolOp, FaceSurface, face_surface, offset_surface, patch_region, planar_regions_boolean, smooth_picks, stitch};

use crate::sheet_surface_convert::{cylinder_chart, cylinder_chart_in, face_region, planar_face};
use crate::{DocError, Result};
use solidcraft_develop::Query;

/// The most faces a body may have to be analysed.
pub const MAX_FACES: usize = 20_000;

/// What to look for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// Smallest and largest thickness to pair across (mm).
    pub min_thickness: f64,
    pub max_thickness: f64,
    /// Pairs whose in-plane span over thickness is below this are flagged for review: a chunky
    /// region is not shell-like, and a shell model of it is a judgement call.
    pub min_aspect: f64,
    /// Slack on distances (mm); 0 picks one from the body's size.
    pub tolerance: f64,
}

impl Default for Options {
    /// The recommended limits: any thickness up to 10 mm, aspect 5, tolerance from the body.
    fn default() -> Options {
        Options { min_thickness: 0.0, max_thickness: 10.0, min_aspect: 5.0, tolerance: 0.0 }
    }
}

/// How far a pair can be trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    /// Shell-like, the faces overlapping on half the smaller one or more, nothing in between.
    High,
    /// Needs a decision: see the pair's notes.
    Review,
}

/// Two faces facing each other across material.
#[derive(Clone, Debug, PartialEq)]
pub struct Pair {
    /// The two faces, as indices into the analysed body's faces.
    pub faces: [usize; 2],
    /// A point inside each face (on it, off its edges): how a design stores the pair, so that a
    /// rebuild finds the same faces without pairing again.
    pub points: [[f64; 3]; 2],
    /// Distance between the faces (mm): the shell's thickness.
    pub thickness: f64,
    /// Area of the sheet the pair gives before its joints are made (mm²).
    pub overlap_area: f64,
    /// Share of the smaller face the overlap covers (0…1).
    pub coverage: f64,
    /// Square root of the overlap area over the thickness: how thin the wall is for its size.
    pub aspect: f64,
    /// [`Confidence::High`]: nothing in the evidence needs a decision, and [`confident`] builds
    /// it. [`Confidence::Review`]: `notes` say what needs a decision; it is built only when a
    /// caller chooses it.
    pub confidence: Confidence,
    /// Why a pair needs review, in words an analyst or an agent can act on.
    pub notes: Vec<String>,
    /// Area of openings in only one of the two faces (a boss's foot, a blind pocket's mouth)
    /// that the sheet is filled over (mm²): the material there is not this wall's to model.
    pub filled_over: f64,
    /// The plane halfway between the faces, its normal toward the first face.
    mid: OutwardPlane,
    /// The first face's plane frame, and its outward normal: what `shared` is read in.
    frame: Plane,
    out: Vec3,
    /// What the sheet covers, in `frame`'s coordinates.
    shared: Vec<Region2>,
    /// The material between the two faces.
    wall: Wall,
    /// For a round wall (two coaxial cylinders): what the sheet is built from. `None` for flat.
    round: Option<Round>,
}

impl Pair {
    /// Whether the wall is round (two coaxial cylinders).
    pub fn is_round(&self) -> bool {
        self.round.is_some()
    }
}

/// A round wall: the outer cylinder face is offset by half the wall inward to make the sheet.
#[derive(Clone, Debug, PartialEq)]
struct Round {
    outer_face: usize,
    /// Whether the two faces cover the same part of the wall (the same length along the axis and
    /// the same angles round it): only then is the whole outer face's offset the wall's sheet.
    whole: bool,
    /// How many holes the outer face has (openings in its chart): the offset carries them all.
    holes: usize,
}

/// Why a face is in no pair.
#[derive(Clone, Debug, PartialEq)]
pub enum Reason {
    /// Neither a plane nor a cylinder: no wall is paired across it.
    NotPlanar,
    /// A face looks back at it, but farther than `max_thickness` (mm).
    OutOfRange { gap: f64 },
    /// A face looks back at it, but nearer than `min_thickness` (mm).
    TooThin { gap: f64 },
    /// A face looks back at it within the thickness limits (`gap` mm away), but the two share no
    /// area seen across the wall: their outlines do not overlap.
    NoOverlap { gap: f64 },
    /// Nothing looks back at it.
    NoFacingFace,
    /// The thin end of a wall: it borders the pairs listed (indices into [`Analysis::pairs`]) and
    /// lies within their walls, so their sheets account for it. Not a gap.
    Rim { pairs: Vec<usize> },
}

/// A face in no pair, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Leftover {
    /// The face, as an index into the analysed body's faces.
    pub face: usize,
    /// Why it is in no pair.
    pub reason: Reason,
    /// For a rim: for the points of its edges, the sets of pairs (indices into
    /// [`Analysis::pairs`]) whose walls hold them, each set once. The rim is covered when every
    /// set has a built pair: a rim two walls share is not covered by one of them alone.
    held_by: Vec<Vec<usize>>,
}

/// What [`analyze`] found: every face of the body is in at least one pair or in exactly one
/// leftover, never both.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Analysis {
    /// The proposed pairs (a face may be in more than one: a flange split by its web).
    pub pairs: Vec<Pair>,
    /// The faces in no pair, each with its reason.
    pub leftovers: Vec<Leftover>,
    /// How many faces the body has.
    pub faces: usize,
    /// The slack the distances were decided with (mm): two thicknesses closer than this are one.
    pub tolerance: f64,
    /// The distances (mm) between two faces facing each other across material past the largest
    /// thickness looked for, thinnest first: the walls the analysis saw but left out. A suggested
    /// limit stays short of the first one it can tell apart from its cluster. Only those a cluster
    /// could still take as its own (within 1% and twice the slack of the limit plus the slack) are
    /// all kept, and the first past them: no cluster can tell a thicker one apart before it.
    pub beyond: Vec<f64>,
}

/// What every pairing step looks at: the body, a query on its record, the options and the
/// distance under which two points count as one.
#[derive(Clone, Copy)]
struct Look<'a> {
    body: &'a Body,
    q: &'a Query<'a>,
    opts: &'a Options,
    tol: f64,
    /// The exact points of every edge of the body (each edge's ends and seven between).
    samples: &'a [[f64; 3]],
}

/// The nearest face looking back at a face, by how it failed: nearer than the thinnest wall
/// asked for, farther than the thickest, or within the limits with no area shared.
#[derive(Clone, Copy, Default)]
struct Nearest {
    too_thin: Option<f64>,
    too_far: Option<f64>,
    no_overlap: Option<f64>,
}

fn nearer(slot: &mut Option<f64>, gap: f64) {
    *slot = Some(slot.map_or(gap, |x| x.min(gap)));
}

/// Records a wall `gap` thick past the analysis's limit in `beyond` (sorted, thinnest first).
/// Every cluster lies within the limit plus the slack, so a wall past `past` (1% and twice the
/// slack beyond that) is told apart from each of them: of those only the thinnest is kept. Those
/// up to `past` are all kept, as some clusters can tell them apart and others cannot.
fn note_beyond(beyond: &mut Vec<f64>, gap: f64, past: f64) {
    if gap > past {
        match beyond.last_mut() {
            Some(last) if *last > past => *last = last.min(gap),
            _ => beyond.push(gap),
        }
        return;
    }
    let at = beyond.partition_point(|t| *t < gap);
    if beyond.get(at) != Some(&gap) {
        beyond.insert(at, gap);
    }
}

/// A planar face as the analysis sees it.
struct Flat {
    plane: Plane,
    out: Vec3,
    region: Region2,
}

/// A cylindrical face as the analysis sees it: its cylinder, with the side its material lies on
/// read from the record (the kernel's `face_surface` does not say which side).
struct RoundFace {
    cylinder: CylinderFace,
    axis: Vec3,
}

fn array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn vec3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn frame_of(p: &Plane) -> Frame {
    Frame { origin: array(p.origin), x: array(p.x), y: array(p.y) }
}

fn ops_err(what: &str) -> impl Fn(solidcraft_kernel::services::Error) -> DocError + '_ {
    move |e| DocError::Invalid(format!("midsurface: {what}: {e}"))
}

/// The faces' edges of face `f`, in its loops' order.
fn face_edges(q: &Query, f: usize) -> Vec<usize> {
    let Some(face) = q.record().faces.get(f) else { return Vec::new() };
    face.loops
        .iter()
        .flat_map(|l| match l {
            Bounds::Edges { edges } => edges.iter().map(|u| u.edge as usize).collect::<Vec<_>>(),
            Bounds::Vertex { .. } => Vec::new(),
        })
        .collect()
}

fn round_face(body: &Body, q: &Query, f: usize, tol: f64) -> Option<RoundFace> {
    let Some(FaceSurface::Cylinder { origin, axis, radius }) = face_surface(body, f) else { return None };
    let axis: Vec3 = *axis;
    // A point of the face (halfway along its first edge), and the face's outward normal there.
    let e = *face_edges(q, f).first()?;
    let [t0, t1] = q.edge_interval(e)?;
    let (p, _) = q.edge_at(e, 0.5 * (t0 + t1))?;
    let n = q.face_normal(f, q.face_uv(f, p, tol)?)?;
    let cylinder = CylinderFace {
        axis_point: array(origin),
        axis: array(axis),
        radius,
        // The kernel reads the side from the outward normal there: pointing away from the axis,
        // the material is toward it.
        material_toward_axis: material_toward_axis(array(origin), array(axis), array(p), array(n)).ok()?,
    };
    Some(RoundFace { cylinder, axis })
}

/// A point inside face `f`, decided on the face by the kernel: a flat face's region's inner
/// point; a round face's first edge's middle, moved a little along the axis or round it, onto
/// whichever side the face lies.
fn inside_point(body: &Body, q: &Query, f: usize, flat: Option<&Flat>, axis: Option<Vec3>, tol: f64) -> Option<[f64; 3]> {
    if let Some(flat) = flat {
        let p = flat.region.interior_point();
        return ms::from_frame(&frame_of(&flat.plane), [p.x, p.y]).ok();
    }
    let axis = axis?;
    let e = *face_edges(q, f).first()?;
    let [t0, t1] = q.edge_interval(e)?;
    let (p, tangent) = q.edge_at(e, 0.5 * (t0 + t1))?;
    let directions = round_directions(array(axis), array(tangent)).ok()?;
    // A narrow wall (a short band, a small radius in a large body) holds no probe a thousandth of
    // the body out: each failed round tries a quarter as far, down to ten times the slack.
    let floor = tol * 10.0;
    std::iter::successors(Some((body.size() * 1e-3).max(floor)), |s| Some(s * 0.25).filter(|s| *s >= floor))
        .find_map(|step| directions.iter().find_map(|d| body.point_lies_on_face(f, p + vec3(*d) * step, step * 0.5).ok().flatten()))
        .map(array)
}

/// The exact points of face `f`'s edges: each edge's ends and seven points between (an arc's
/// bulge shows). `None` when an edge cannot be read.
fn edge_points(q: &Query, f: usize) -> Option<Vec<[f64; 3]>> {
    let mut points = Vec::new();
    for e in face_edges(q, f) {
        let [t0, t1] = q.edge_interval(e)?;
        for k in 0..=8 {
            let t = t0 + (t1 - t0) * (f64::from(k) / 8.0);
            points.push(array(q.edge_at(e, t)?.0));
        }
    }
    Some(points)
}

/// The area regions `x` and `y` (outlines without holes, charted on one cylinder chart) have in
/// common round the axis. The charts unwrap each outline on its own turn, so `y` is also compared
/// a turn either way; the three turned copies do not overlap, so their shares add up.
fn common_round(x: &[Region2], y: &[Region2], slack: f64, what: &str) -> Result<f64> {
    use solidcraft_develop::prelude::*;
    if x.is_empty() || y.is_empty() {
        return Ok(0.0);
    }
    let mut common = 0.0;
    for turns in [-1.0, 0.0, 1.0] {
        let turned = solidcraft_develop::Rigid2 { cos: 1.0, sin: 0.0, shift: Vec2::new(0.0, turns * std::f64::consts::TAU) };
        let moved: Vec<Region2> = y.iter().map(|r| Region2 { outer: turned.loop2(&r.outer), holes: Vec::new() }).collect();
        let shared = planar_regions_boolean(x, &moved, BoolOp::Intersect, slack)
            .map_err(|e| DocError::Invalid(format!("midsurface: {what} compared round their axis: {e}")))?;
        common += shared.iter().map(|r| r.area()).sum::<f64>();
    }
    Ok(common)
}

/// Whether every loop of each of the two faces `faces` reaches the other face, read on the
/// body's topology (no geometry): some edge of the loop is shared with the other face, or with a
/// face that borders it. A hole drilled through the wall has its bore's face on both; a blind
/// pocket's mouth (or a boss's foot) leads only to its own side and floor, so it is an opening in
/// one face only, which the outer face's offset would carry as a hole the wall has not. (A chart
/// cannot be used here: a hole a drill cuts in a cylinder is a curve no chart loop holds.)
fn openings_match(q: &Query, faces: [usize; 2]) -> bool {
    let borders = |g: usize, other: usize| face_edges(q, g).into_iter().any(|e| q.edge_faces(e).contains(&other));
    let reaches = |f: usize, other: usize| {
        let Some(face) = q.record().faces.get(f) else { return false };
        face.loops.iter().all(|l| match l {
            Bounds::Edges { edges } => {
                edges.iter().any(|u| q.edge_faces(u.edge as usize).into_iter().any(|g| g == other || (g != f && borders(g, other))))
            }
            Bounds::Vertex { .. } => true,
        })
    };
    reaches(faces[0], faces[1]) && reaches(faces[1], faces[0])
}

/// The body's round faces: each face's cylinder (`None` for the others), and the round ones as
/// (radius, index) sorted by radius, so that "is anything between these two radii?" looks only
/// between them.
struct Rounds {
    faces: Vec<Option<RoundFace>>,
    radii: Vec<(f64, usize)>,
}

impl Rounds {
    fn new(faces: Vec<Option<RoundFace>>) -> Rounds {
        let mut radii: Vec<(f64, usize)> = faces.iter().enumerate().filter_map(|(g, c)| c.as_ref().map(|c| (c.cylinder.radius, g))).collect();
        radii.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        Rounds { faces, radii }
    }
}

/// A face among `rounds` (not one of `faces`) on the wall's axis whose radius lies strictly
/// between the wall's `inner` and `outer` and whose outline overlaps the first face's `chart`
/// (its outline, holes filled) round the axis: a gap or another wall between the two faces, not
/// material. Its index and radius. Only the faces between the two radii are looked at, nearest
/// the inner first: nested tubes do not make every pair read every cylinder.
fn round_between(look: &Look, rounds: &Rounds, faces: [usize; 2], chart: &Region2, inner: f64, outer: f64) -> Result<Option<(usize, f64)>> {
    let Look { body, q, tol, .. } = *look;
    let Some(Some(a)) = rounds.faces.get(faces[0]) else { return Ok(None) };
    // The kernel decides "on the same axis": `c` taken as an outer face faces the inner one.
    let inside = CylinderFace { radius: inner, material_toward_axis: false, ..a.cylinder };
    let slack = tol.clamp(1e-9, 1e-4);
    let from = rounds.radii.partition_point(|(r, _)| *r <= inner + tol);
    let candidates = rounds.radii.iter().skip(from).take_while(|(r, _)| *r < outer - tol);
    for (g, c) in candidates.filter_map(|(_, g)| rounds.faces.get(*g).and_then(Option::as_ref).map(|c| (*g, c))) {
        if faces.contains(&g) {
            continue;
        }
        let probe = CylinderFace { material_toward_axis: true, ..c.cylinder };
        if facing_cylinders(&inside, &probe).map_err(|e| ops_err(&format!("faces {} and {g}", faces[0]))(e))?.is_none() {
            continue;
        }
        let Some(chart_c) = cylinder_chart_in(body, q, g, faces[0], tol) else { continue };
        let what = format!("faces {} and {g}", faces[0]);
        if common_round(std::slice::from_ref(chart), &[filled(&chart_c)], slack, &what)? > tol * tol {
            return Ok(Some((g, c.cylinder.radius)));
        }
    }
    Ok(None)
}

/// What lies inside the round wall between faces `faces` (radii `inner` and `outer` about
/// `cylinder`'s axis), of any surface: a void or another face the sheet would span, as a note for
/// review; `None` when the wall is clear. Any other face has edges there: an exact point of an
/// edge strictly between the two radii (more than a margin from each) and well within the wall (the
/// kernel finds the nearest points of both faces a thickness apart through it, and through it moved
/// a quarter of the thickness along the axis or round it) is one. The edges of the two faces
/// and of every face bordering them are the wall's rims and bores, not inside it. An edge or a
/// point the kernel cannot read leaves the wall unchecked, which is said too.
fn edge_in_round_wall(look: &Look, faces: [usize; 2], outer_face: usize, cylinder: &CylinderFace, inner: f64, outer: f64) -> Option<String> {
    let Look { body, q, tol, .. } = *look;
    let inner_face = if faces[0] == outer_face { faces[1] } else { faces[0] };
    let margin = (tol * 100.0).min((outer - inner) * 0.25);
    let shell = Wall::Round { axis_point: cylinder.axis_point, axis: cylinder.axis, inner: inner + margin, outer: outer - margin };
    let mut near: Vec<usize> = faces.to_vec();
    for f in faces {
        for e in face_edges(q, f) {
            near.extend(q.edge_faces(e));
        }
    }
    let unchecked =
        |what: String| Some(format!("unchecked: {what} could not be tested for whether it lies inside the round wall of faces {faces:?}"));
    let Some(axis) = vec3(cylinder.axis).normalized() else { return unchecked("its axis".to_string()) };
    let nudge = (outer - inner) * 0.25;
    let reach = outer - inner + tol;
    // Inside the wall, the nearest points of the two faces lie on one line through the point,
    // the wall's thickness apart: past the wall's end (beside it) they are its rims, and farther.
    // The outer face's nearest point when the point is inside; `Err` when the kernel cannot tell.
    let within = |p: Vec3| -> std::result::Result<Option<Vec3>, ()> {
        let on_outer = body.point_lies_on_face(outer_face, p, reach).map_err(|_| ())?;
        let on_inner = body.point_lies_on_face(inner_face, p, reach).map_err(|_| ())?;
        Ok(on_outer.zip(on_inner).filter(|(o, i)| p.dist(*o) + p.dist(*i) <= outer - inner + tol).map(|(o, _)| o))
    };
    for e in 0..q.record().edges.len() {
        if q.edge_faces(e).into_iter().any(|g| near.contains(&g)) {
            continue;
        }
        let Some([t0, t1]) = q.edge_interval(e) else { return unchecked(format!("edge {e}")) };
        for k in 0..=8 {
            let Some((p, _)) = q.edge_at(e, t0 + (t1 - t0) * (f64::from(k) / 8.0)) else { return unchecked(format!("edge {e}")) };
            match ms::wall_holds(&shell, &[array(p)], 0.0) {
                Ok(false) => continue,
                Ok(true) => {}
                Err(_) => return unchecked(format!("edge {e}")),
            }
            let o = match within(p) {
                Ok(Some(o)) => o,
                Ok(None) => continue,
                Err(()) => return unchecked(format!("edge {e}")),
            };
            // Well inside, not on the wall's end or its sides: moved a quarter of the thickness
            // along the axis or round it, either way, the point is still inside.
            let Some(round) = axis.cross(o - p).normalized() else { return unchecked(format!("edge {e}")) };
            let mut all = true;
            for d in [axis, round] {
                for moved in [p + d * nudge, p - d * nudge] {
                    match within(moved) {
                        Ok(Some(_)) => {}
                        Ok(None) => all = false,
                        Err(()) => return unchecked(format!("edge {e}")),
                    }
                }
            }
            if all {
                return Some(format!(
                    "blocked: an edge of the body runs inside the round wall of faces {faces:?} at ({:.3}, {:.3}, {:.3}): a void or another face lies between them",
                    p.x, p.y, p.z
                ));
            }
        }
    }
    None
}

/// A chart region's extent: its x range and its y range.
fn extent(r: &Region2, tol: f64) -> Option<([f64; 2], [f64; 2])> {
    let poly = r.outer.polyline(tol);
    let mut it = poly.iter();
    let first = it.next()?;
    let (mut x, mut y) = ([first.x, first.x], [first.y, first.y]);
    for p in it {
        x = [x[0].min(p.x), x[1].max(p.x)];
        y = [y[0].min(p.y), y[1].max(p.y)];
    }
    Some((x, y))
}

/// The pair of two coaxial cylinders `a` and `b`, or `None` when their outlines, read on one
/// chart, share no area. The sheet is the outer face offset half the wall inward, so only faces
/// with the same extent along the axis and the same sweep round it (a tube, a bend) are trusted:
/// others are listed for review, not built.
fn round_pair(
    look: &Look,
    rounds: &Rounds,
    faces: [usize; 2],
    a: &RoundFace,
    b: &RoundFace,
    thickness: f64,
    mid_radius: f64,
) -> Result<Option<Pair>> {
    let Look { body, q, opts, tol, .. } = *look;
    let outer_face = if a.cylinder.material_toward_axis { faces[0] } else { faces[1] };
    let mut notes = Vec::new();
    let len_tol = (body.size() * 1e-5).max(1e-6);
    let slack = tol.clamp(1e-9, 1e-4);
    let (inner, outer) = (a.cylinder.radius.min(b.cylinder.radius), a.cylinder.radius.max(b.cylinder.radius));
    let (ca, cb) = (cylinder_chart(body, q, faces[0], tol), cylinder_chart(body, q, faces[1], tol));
    let mut area = 0.0;
    let mut coverage = 0.0;
    let mut whole = false;
    let outer_chart = if outer_face == faces[0] { ca.as_ref() } else { cb.as_ref() };
    let holes = outer_chart.map_or(0, |r| r.holes.len());
    let outline_a = ca.as_ref().and_then(|r| extent(r, tol).map(|e| (r, e)));
    // `b`'s axial range read in `a`'s coordinates (its origin may differ, and its axis run the
    // other way), by the kernel.
    let outline_b = cb
        .as_ref()
        .and_then(|r| extent(r, tol).map(|e| (r, e)))
        .and_then(|(r, (x, y))| axial_range_in(&a.cylinder, &b.cylinder, x).ok().map(|x| (r, (x, y))));
    match (outline_a, outline_b) {
        (Some((ra, (xa, ya))), Some((rb, (xb, yb)))) => {
            // The outlines (holes filled: an opening through both does not change which part of
            // the wall they cover), both on the first face's chart: the share of the smaller the
            // two have in common. The same sweep is not the same angles (0 to 90 degrees against
            // 45 to 135), and a whole turn's extent along the axis may differ from angle to
            // angle, so every pair is compared on the chart.
            let b_in_a = cylinder_chart_in(body, q, faces[1], faces[0], tol);
            let (fa, fb) = (filled(ra), filled(rb));
            let common = match &b_in_a {
                Some(r) => common_round(std::slice::from_ref(&fa), &[filled(r)], slack, &format!("the round faces {faces:?}"))?,
                None => 0.0,
            };
            // Both outlines read and nothing in common (a bore and an outside at different
            // places along the axis): no wall between them, as for two flat faces.
            if b_in_a.is_some() && common * mid_radius <= tol * tol {
                return Ok(None);
            }
            let (smaller, larger) = (fa.area().min(fb.area()), fa.area().max(fb.area()));
            coverage = (common / smaller.max(f64::MIN_POSITIVE)).clamp(0.0, 1.0);
            let (sweep_a, sweep_b) = (ya[1] - ya[0], yb[1] - yb[0]);
            let same_axial = (xa[0] - xb[0]).abs() <= len_tol && (xa[1] - xb[1]).abs() <= len_tol;
            let same_sweep = (sweep_a - sweep_b).abs() <= 1e-4;
            let same_angles = larger > 0.0 && common >= larger * (1.0 - 1e-3);
            let aligned = same_axial && same_sweep && same_angles;
            // An opening in one face only has no partner across the wall: the outer face's offset
            // would carry it as a hole the wall does not have (or miss one it has).
            let matched = openings_match(q, faces);
            whole = aligned && matched;
            // A whole wall's sheet is the outer face's offset, holes and all; of any other pair,
            // what the two outlines share (it is not built, and no more of the wall is in it).
            area = if whole { outer_chart.map_or(0.0, |r| r.area()) } else { common };
            if aligned && !matched {
                notes.push(format!(
                    "opening: faces {faces:?} have openings that do not match across the wall (a blind pocket or a boss in one face); a round wall with one is not built yet"
                ));
            }
            if let Some((g, r)) = round_between(look, rounds, faces, &fa, inner, outer)? {
                notes.push(format!(
                    "blocked: face {g}, round at radius {r:.3} on the same axis, lies between radii {inner:.3} and {outer:.3}: the two faces may be separate walls with a gap between"
                ));
            } else if let Some(note) = edge_in_round_wall(look, faces, outer_face, &a.cylinder, inner, outer) {
                notes.push(note);
            }
            if !aligned {
                notes.push(format!(
                    "partial: the two faces differ in length along the axis ({:.3} to {:.3} against {:.3} to {:.3}), in the angles they sweep ({:.4} against {:.4} rad) or in how much of the wall they cover ({:.2}% in common); a sheet for part of a round wall is not built yet",
                    xa[0],
                    xa[1],
                    xb[0],
                    xb[1],
                    sweep_a,
                    sweep_b,
                    coverage * 100.0
                ));
            }
        }
        _ => notes.push("the faces' outlines could not be read round the axis".to_string()),
    }
    let overlap_area = area * mid_radius;
    let aspect = overlap_area.sqrt() / thickness;
    if aspect < opts.min_aspect {
        notes.push(format!(
            "chunky: spans about {:.1} times its thickness ({:.3} mm), below {:.1}; a shell may not represent it",
            aspect, thickness, opts.min_aspect
        ));
    }
    let confidence = if notes.is_empty() { Confidence::High } else { Confidence::Review };
    let (pa, pb) = (inside_point(body, q, faces[0], None, Some(a.axis), tol), inside_point(body, q, faces[1], None, Some(b.axis), tol));
    // As for two flat faces: a pair that cannot be named is left out, and the scan goes on.
    let (Some(pa), Some(pb)) = (pa, pb) else { return Ok(None) };
    Ok(Some(Pair {
        faces,
        points: [pa, pb],
        thickness,
        overlap_area,
        coverage,
        aspect,
        confidence,
        notes,
        filled_over: 0.0,
        mid: OutwardPlane { point: a.cylinder.axis_point, outward: a.cylinder.axis },
        frame: Plane::XY,
        out: Vec3::new(0.0, 0.0, 0.0),
        shared: Vec::new(),
        wall: Wall::Round { axis_point: a.cylinder.axis_point, axis: a.cylinder.axis, inner, outer },
        round: Some(Round { outer_face, whole, holes }),
    }))
}

/// The face's plane as the kernel's pairing takes it.
fn outward(f: &Flat) -> OutwardPlane {
    OutwardPlane { point: array(f.plane.origin), outward: array(f.out) }
}

fn check(opts: &Options) -> Result<()> {
    let bad = |m: &str| DocError::Invalid(format!("midsurface: {m}"));
    if !(opts.min_thickness.is_finite() && opts.max_thickness.is_finite() && opts.min_aspect.is_finite() && opts.tolerance.is_finite()) {
        return Err(bad("every limit must be a finite number"));
    }
    if opts.min_thickness < 0.0 || opts.max_thickness <= opts.min_thickness || opts.min_aspect < 0.0 || opts.tolerance < 0.0 {
        return Err(bad("thickness must run from 0 or more up to a larger value, and the other limits cannot be negative"));
    }
    Ok(())
}

/// The slack the analysis decides distances with.
fn slack_of(body: &Body, opts: &Options) -> f64 {
    if opts.tolerance > 0.0 { opts.tolerance } else { (body.size() * 1e-6).max(1e-6) }
}

/// The pairs and leftovers of `body`. Errors only for a hostile request or a body that cannot be
/// read; a body with nothing to pair gives an `Analysis` of leftovers.
pub fn analyze(body: &Body, opts: &Options) -> Result<Analysis> {
    let bad = |m: &str| DocError::Invalid(format!("midsurface: {m}"));
    check(opts)?;
    if body.is_mesh() {
        return Err(bad("a mesh body has no exact faces to pair"));
    }
    let n = body.face_count();
    if n > MAX_FACES {
        return Err(bad("too many faces to pair"));
    }
    let tol = slack_of(body, opts);
    let q = Query::new(body.record());
    // Every edge's exact points, read once: what the blocking test looks for between two faces.
    let mut samples: Vec<[f64; 3]> = Vec::new();
    for e in 0..body.record().edges.len() {
        let Some([t0, t1]) = q.edge_interval(e) else { continue };
        for k in 0..=8 {
            if let Some((p, _)) = q.edge_at(e, t0 + (t1 - t0) * (f64::from(k) / 8.0)) {
                samples.push(array(p));
            }
        }
    }
    let look = Look { body, q: &q, opts, tol, samples: &samples };
    let flats: Vec<Option<Flat>> = (0..n)
        .map(|f| {
            let (plane, out) = planar_face(body, &q, f, tol)?;
            let region = face_region(body, &q, &plane, out, f, tol)?;
            Some(Flat { plane, out, region })
        })
        .collect();

    let mut pairs: Vec<Pair> = Vec::new();
    let mut nearest: Vec<Nearest> = vec![Nearest::default(); n];
    let mut beyond: Vec<f64> = Vec::new();
    // Past this, a wall is told apart from every cluster (see `thickness_clusters`).
    let past = (opts.max_thickness + tol) * 1.01 + 1e-6 + 2.0 * tol;
    // Records, for faces `f` and `g`, a face looking back `gap` away that made no pair.
    let mut missed = |f: usize, g: usize, gap: f64| {
        if gap > opts.max_thickness + tol {
            note_beyond(&mut beyond, gap, past);
        }
        for slot in [f, g] {
            if let Some(near) = nearest.get_mut(slot) {
                let which = if gap < opts.min_thickness - tol {
                    &mut near.too_thin
                } else if gap > opts.max_thickness + tol {
                    &mut near.too_far
                } else {
                    &mut near.no_overlap
                };
                nearer(which, gap);
            }
        }
    };
    for (f, a) in flats.iter().enumerate() {
        let Some(a) = a else { continue };
        for (g, b) in flats.iter().enumerate().skip(f + 1) {
            let Some(b) = b else { continue };
            // The kernel decides whether the two planes face each other across material, and
            // how far apart they are.
            // The message is made only on an error: this runs for every two flat faces.
            let facing = facing_planes(&outward(a), &outward(b)).map_err(|e| ops_err(&format!("faces {f} and {g}"))(e))?;
            let Some(facing) = facing else { continue };
            let gap = facing.thickness;
            if gap < opts.min_thickness - tol || gap > opts.max_thickness + tol {
                missed(f, g, gap);
                continue;
            }
            let mid = OutwardPlane { point: facing.mid_point, outward: facing.toward_first };
            match pair_of(&look, [f, g], a, b, gap, mid)? {
                Some(p) => pairs.push(p),
                None => missed(f, g, gap),
            }
        }
    }
    // Round walls: two coaxial cylinders, the outer's material toward the axis, the inner's away.
    let rounds = Rounds::new((0..n).map(|f| if flats.get(f).is_some_and(Option::is_some) { None } else { round_face(body, &q, f, tol) }).collect());
    for (f, a) in rounds.faces.iter().enumerate() {
        let Some(a) = a else { continue };
        for (g, b) in rounds.faces.iter().enumerate().skip(f + 1) {
            let Some(b) = b else { continue };
            let facing = facing_cylinders(&a.cylinder, &b.cylinder).map_err(|e| ops_err(&format!("faces {f} and {g}"))(e))?;
            let Some(facing) = facing else { continue };
            if facing.thickness < opts.min_thickness - tol || facing.thickness > opts.max_thickness + tol {
                missed(f, g, facing.thickness);
                continue;
            }
            match round_pair(&look, &rounds, [f, g], a, b, facing.thickness, facing.mid_radius)? {
                Some(p) => pairs.push(p),
                None => missed(f, g, facing.thickness),
            }
        }
    }
    rivalries(&mut pairs, tol)?;
    let in_pair = |f: usize| pairs.iter().any(|p| p.faces.contains(&f));
    let mut leftovers = Vec::new();
    for face in (0..n).filter(|f| !in_pair(*f)) {
        let flat = flats.get(face).and_then(Option::as_ref);
        let round = rounds.faces.get(face).and_then(Option::as_ref);
        let (rim, held_by) = if flat.is_some() || round.is_some() { rim_of(&q, face, &pairs, tol)? } else { (Vec::new(), Vec::new()) };
        let reason = if !rim.is_empty() {
            Reason::Rim { pairs: rim }
        } else if flat.is_none() && round.is_none() {
            Reason::NotPlanar
        } else {
            // Within the limits but sharing nothing says most; then too far, then too thin.
            match nearest.get(face).copied().unwrap_or_default() {
                Nearest { no_overlap: Some(gap), .. } => Reason::NoOverlap { gap },
                Nearest { too_far: Some(gap), .. } => Reason::OutOfRange { gap },
                Nearest { too_thin: Some(gap), .. } => Reason::TooThin { gap },
                Nearest { .. } => Reason::NoFacingFace,
            }
        };
        leftovers.push(Leftover { face, reason, held_by });
    }
    Ok(Analysis { pairs, leftovers, faces: n, tolerance: tol, beyond })
}

/// Pairs that share a face are fine when they cover different parts of it (a flange split by its
/// web); they compete when what they share overlaps (a stepped wall): that is a decision.
fn rivalries(pairs: &mut [Pair], tol: f64) -> Result<()> {
    let slack = tol.clamp(1e-9, 1e-4);
    let mut rivalry: Vec<Vec<String>> = vec![Vec::new(); pairs.len()];
    // Only pairs that share a face can compete: the flat pairs of each face, compared within.
    let mut by_face: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, p) in pairs.iter().enumerate().filter(|(_, p)| p.round.is_none()) {
        for f in p.faces {
            by_face.entry(f).or_default().push(i);
        }
    }
    // Each comparison is an exact region boolean: past this many, the pairs of a crowded face
    // (a stack of plates within one thickness limit) are left for a decision, not compared.
    let mut budget: usize = 200_000;
    let mut checks: Vec<(usize, usize)> = Vec::new();
    for (f, list) in &by_face {
        let count = list.len().saturating_sub(1).saturating_mul(list.len()) / 2;
        if count > budget {
            for i in list {
                if let Some(slot) = rivalry.get_mut(*i) {
                    slot.push(format!(
                        "shares face {f} with {} other pairs, too many to compare: choose which to build",
                        list.len().saturating_sub(1)
                    ));
                }
            }
            continue;
        }
        budget -= count;
        for (k, i) in list.iter().enumerate() {
            for j in list.iter().skip(k + 1) {
                // Two pairs sharing both faces meet in two lists: compared once, in the first.
                let (Some(p), Some(r)) = (pairs.get(*i), pairs.get(*j)) else { continue };
                if p.faces.iter().filter(|g| r.faces.contains(g)).min() == Some(f) {
                    checks.push((*i, *j));
                }
            }
        }
    }
    checks.sort_unstable();
    for (i, j) in checks {
        let (Some(p), Some(r)) = (pairs.get(i), pairs.get(j)) else { continue };
        let note = match shared_overlap(p, r, slack)? {
            Some(area) if area <= tol * tol => continue,
            Some(area) => {
                format!("competes with the pair of faces {:?}: they share {area:.1} mm² of the same wall; choose which to build", r.faces)
            }
            None => format!("could not be compared with the pair of faces {:?} (curved edges)", r.faces),
        };
        let back = note.replace(&format!("{:?}", r.faces), &format!("{:?}", p.faces));
        if let Some(slot) = rivalry.get_mut(i) {
            slot.push(note);
        }
        if let Some(slot) = rivalry.get_mut(j) {
            slot.push(back);
        }
    }
    for (p, notes) in pairs.iter_mut().zip(rivalry) {
        if !notes.is_empty() {
            p.confidence = Confidence::Review;
            p.notes.extend(notes);
        }
    }
    Ok(())
}

/// The pairs (indices) whose walls hold face `face`, when it borders them and every point of its
/// edges lies in one of them: the face is no wider than those walls, their rim. With them, the
/// distinct sets of those pairs that hold each point (see [`Leftover`]). Empty otherwise.
fn rim_of(q: &Query, face: usize, pairs: &[Pair], tol: f64) -> Result<(Vec<usize>, Vec<Vec<usize>>)> {
    let none = || (Vec::new(), Vec::new());
    let edges = face_edges(q, face);
    let mut near: Vec<usize> = Vec::new();
    for e in &edges {
        for g in q.edge_faces(*e) {
            for (i, p) in pairs.iter().enumerate() {
                if g != face && p.faces.contains(&g) && !near.contains(&i) {
                    near.push(i);
                }
            }
        }
    }
    if near.is_empty() {
        return Ok(none());
    }
    // The edges' exact points: their ends and seven between (an arc's bulge shows).
    let Some(points) = edge_points(q, face) else { return Ok(none()) };
    if points.is_empty() {
        return Ok(none());
    }
    let slack = tol * 100.0;
    let mut held_by: Vec<Vec<usize>> = Vec::new();
    for p in &points {
        let mut holders = Vec::new();
        for i in &near {
            let Some(pair) = pairs.get(*i) else { continue };
            if ms::wall_holds(&pair.wall, std::slice::from_ref(p), slack).map_err(ops_err(&format!("the rim test of face {face}")))? {
                holders.push(*i);
            }
        }
        if holders.is_empty() {
            return Ok(none());
        }
        if !held_by.contains(&holders) {
            held_by.push(holders);
        }
    }
    Ok((near, held_by))
}

/// What `q` shares, read in `to`'s coordinates; `None` when it has a curved edge.
fn in_frame(q: &Pair, to: &Plane) -> Option<Vec<Region2>> {
    let (from, to) = (frame_of(&q.frame), frame_of(to));
    let map = |lp: &Loop2| -> Option<Loop2> {
        let pts: Option<Vec<Vec2>> = lp
            .segs
            .iter()
            .map(|s| match s {
                Seg2::Line { a, .. } => {
                    let p = ms::from_frame(&from, [a.x, a.y]).ok()?;
                    let [u, v] = ms::in_frame(&to, p).ok()?;
                    Some(Vec2::new(u, v))
                }
                _ => None,
            })
            .collect();
        pts.map(|v| Loop2::polygon(&v))
    };
    q.shared
        .iter()
        .map(|r| {
            let outer = map(&r.outer)?.ccw();
            let holes = r.holes.iter().map(|h| map(h).map(|l| l.ccw().reversed())).collect::<Option<Vec<_>>>()?;
            Some(Region2 { outer, holes })
        })
        .collect()
}

/// The area two pairs' shared regions have in common (`None` when they cannot be compared).
fn shared_overlap(p: &Pair, q: &Pair, slack: f64) -> Result<Option<f64>> {
    let Some(theirs) = in_frame(q, &p.frame) else { return Ok(None) };
    let Some(ours) = in_frame(p, &p.frame) else { return Ok(None) };
    let cut = planar_regions_boolean(&ours, &theirs, BoolOp::Intersect, slack)
        .map_err(|e| DocError::Invalid(format!("midsurface: overlap of pairs {:?} and {:?}: {e}", p.faces, q.faces)))?;
    Ok(Some(cut.iter().map(|r| r.area()).sum()))
}

/// A region's outline alone, its holes filled.
fn filled(r: &Region2) -> Region2 {
    Region2 { outer: r.outer.clone(), holes: Vec::new() }
}

/// A region's holes, each as a region of its own.
fn openings(r: &Region2) -> Vec<Region2> {
    r.holes.iter().map(|h| Region2 { outer: h.ccw(), holes: Vec::new() }).collect()
}

/// Whether `p` lies in `r` and at least `margin` from each of its edges (not on its outline or a
/// hole's, where a wall's own rim and bores have their edges).
fn well_inside(r: &Region2, p: Vec2, margin: f64) -> bool {
    r.contains(p) && r.loops().flat_map(|l| l.segs.iter()).all(|s| s.point_at(s.closest_param(p)).dist(p) >= margin)
}

/// The first of `samples` (exact points of the body's edges) that lies strictly between the two
/// faces of a wall `gap` thick, more than `margin` from either, and well inside `shared` (in face
/// `a`'s frame). `mid` is the plane halfway between them; the kernel's wall test reads the two
/// halves of the wall either side of it, each `margin` short of its face. `None` when there is
/// none.
fn edge_between(samples: &[[f64; 3]], a: &Flat, mid: &OutwardPlane, gap: f64, shared: &[Region2], margin: f64) -> Option<[f64; 3]> {
    let margin = margin.min(gap * 0.25);
    let half = gap * 0.5 - margin;
    let [x, y, z] = mid.outward;
    let halves =
        [Wall::Flat { face: *mid, thickness: half }, Wall::Flat { face: OutwardPlane { point: mid.point, outward: [-x, -y, -z] }, thickness: half }];
    let frame = frame_of(&a.plane);
    samples.iter().copied().find(|s| {
        let inside = halves.iter().any(|w| ms::wall_holds(w, std::slice::from_ref(s), 0.0).unwrap_or(false));
        if !inside {
            return false;
        }
        let Ok([u, v]) = ms::in_frame(&frame, *s) else { return false };
        shared.iter().any(|r| well_inside(r, Vec2::new(u, v), margin))
    })
}

/// The pair of `a` (face `faces[0]`) and `b`, or `None` when they share no area. The sheet covers
/// what the two faces' outlines share, less the openings through both (a hole); an opening in one
/// face only (a boss's foot, a blind pocket), or in both with material between, is filled over
/// and counted in `filled_over`.
fn pair_of(look: &Look, faces: [usize; 2], a: &Flat, b: &Flat, gap: f64, mid: OutwardPlane) -> Result<Option<Pair>> {
    let Look { body, q, opts, tol, samples } = *look;
    // `b` in `a`'s coordinates.
    let Some(b_in_a) = face_region(body, q, &a.plane, b.out, faces[1], tol) else { return Ok(None) };
    let slack = tol.clamp(1e-9, 1e-4);
    let boolean = |x: &[Region2], y: &[Region2], op: BoolOp| {
        planar_regions_boolean(x, y, op, slack)
            .map_err(|e| DocError::Invalid(format!("midsurface: overlap of faces {} and {}: {e}", faces[0], faces[1])))
    };
    // The first face (not the pair's own) the kernel's exact ray from `a`'s plane at `p` (in its
    // frame) meets before the partner: its depth and index, or the face it could not test.
    let between = |p: Vec2| -> std::result::Result<Option<(f64, usize)>, usize> {
        let Ok(on_a) = ms::from_frame(&frame_of(&a.plane), [p.x, p.y]) else { return Ok(None) };
        let hits = q.ray(vec3(on_a), -a.out, tol.max(1e-6))?;
        Ok(hits.iter().find(|(s, f, _)| *s > tol && *s < gap - tol && *f != faces[0] && *f != faces[1]).map(|(s, f, _)| (*s, *f)))
    };
    let mut notes = Vec::new();
    let outline = boolean(&[filled(&a.region)], &[filled(&b_in_a)], BoolOp::Intersect)?;
    let (holes_a, holes_b) = (openings(&a.region), openings(&b_in_a));
    let mouths = if holes_a.is_empty() || holes_b.is_empty() { Vec::new() } else { boolean(&holes_a, &holes_b, BoolOp::Intersect)? };
    // Openings in both faces are a hole only where nothing lies between them: two blind pockets
    // facing each other across a floor of material are filled over, not cut out. One ray sees one
    // point: an opening that narrows between the faces (counterbores either side of a smaller
    // bore) has edges of the body strictly between the faces well inside the mouth; it is filled
    // over and listed for review rather than cut out whole.
    let mut through = Vec::new();
    for r in mouths {
        match between(r.interior_point()) {
            Ok(None) => match edge_between(samples, a, &mid, gap, std::slice::from_ref(&r), tol * 100.0) {
                None => through.push(r),
                Some(at) => notes.push(format!(
                    "stepped: an opening through faces {} and {} narrows between them (an edge of the body at ({:.3}, {:.3}, {:.3})); it is filled over, not cut out",
                    faces[0], faces[1], at[0], at[1], at[2]
                )),
            },
            Ok(Some(_)) => {}
            Err(f) => notes.push(format!("unchecked: face {f} could not be tested exactly for whether an opening goes through; it is filled over")),
        }
    }
    let shared = if through.is_empty() || outline.is_empty() { outline } else { boolean(&outline, &through, BoolOp::Cut)? };
    let overlap_area: f64 = shared.iter().map(|r| r.area()).sum();
    if overlap_area <= tol * tol {
        return Ok(None);
    }
    let exact: f64 = boolean(std::slice::from_ref(&a.region), std::slice::from_ref(&b_in_a), BoolOp::Intersect)?.iter().map(|r| r.area()).sum();
    let filled_over = if overlap_area - exact > tol * tol { overlap_area - exact } else { 0.0 };
    let smaller = a.region.area().min(b_in_a.area()).max(f64::MIN_POSITIVE);
    let coverage = (exact / smaller).clamp(0.0, 1.0);
    let aspect = overlap_area.sqrt() / gap;
    if aspect < opts.min_aspect {
        notes.push(format!(
            "chunky: spans about {:.1} times its thickness ({:.3} mm), below {:.1}; a shell may not represent it",
            aspect, gap, opts.min_aspect
        ));
    }
    if coverage < 0.5 {
        notes.push(format!("partial: the faces overlap on only {:.0}% of the smaller one", coverage * 100.0));
    }
    // Anything solid-side between the two faces, seen from the middle of the biggest overlap?
    let biggest = shared.iter().max_by(|x, y| x.area().total_cmp(&y.area()));
    let mut blocked = false;
    if let Some(r) = biggest {
        match between(r.interior_point()) {
            Ok(Some((s, f))) => {
                blocked = true;
                notes.push(format!("blocked: face {f} lies {s:.3} mm below face {} before the partner is reached", faces[0]));
            }
            Ok(None) => {}
            Err(f) => notes.push(format!("unchecked: face {f} could not be tested for blocking exactly")),
        }
    }
    // One ray sees one point. Any other face between the two (a web under one point, a skin
    // inside) has edges there: an exact point of the body's edges strictly between the two
    // planes and well inside the shared region is material, or a void, the sheet would span.
    if !blocked && let Some(at) = edge_between(samples, a, &mid, gap, &shared, tol * 100.0) {
        notes.push(format!(
            "blocked: an edge of the body runs between faces {} and {} at ({:.3}, {:.3}, {:.3}), inside the wall the sheet would span",
            faces[0], faces[1], at[0], at[1], at[2]
        ));
    }
    let confidence = if notes.is_empty() { Confidence::High } else { Confidence::Review };
    let point_b = face_region(body, q, &b.plane, b.out, faces[1], tol)
        .and_then(|r| ms::from_frame(&frame_of(&b.plane), [r.interior_point().x, r.interior_point().y]).ok());
    let point_a = inside_point(body, q, faces[0], Some(a), None, tol);
    let (Some(pa), Some(pb)) = (point_a, point_b) else { return Ok(None) };
    let wall = Wall::Flat { face: outward(a), thickness: gap };
    Ok(Some(Pair {
        faces,
        points: [pa, pb],
        thickness: gap,
        overlap_area,
        coverage,
        aspect,
        confidence,
        notes,
        filled_over,
        mid,
        frame: a.plane,
        out: a.out,
        shared,
        wall,
        round: None,
    }))
}

/// A mid-sheet: the part of the plane halfway between a pair of faces that the pair shares, as a
/// polygon, with its edges at the joints with other walls' sheets moved onto the line the two
/// halfway planes meet in.
#[derive(Clone, Debug)]
pub struct Sheet {
    /// The sheet already built as a body: a round wall's own face, offset to halfway. Flat sheets
    /// are built from their outline when the surface body is made.
    pub body: Option<Body>,
    /// The pair of faces it lies between.
    pub faces: [usize; 2],
    /// The wall's thickness (mm), for the shell's property.
    pub thickness: f64,
    /// The polygon's corners, in order (a curved outline's segments' start points).
    pub corners: Vec<[f64; 3]>,
    /// The outline as the shared region has it, in `frame`'s coordinates.
    pub outline: Loop2,
    /// Whether the outline has curved edges. Such a sheet is built as it is: its joints with other
    /// sheets are not extended or trimmed, and the sheet says so in its notes.
    pub curved: bool,
    /// Whether the joints left nothing of the sheet (a wall no taller than the plate it stands
    /// in): it is not built, and the walls it joins hold its material.
    pub collapsed: bool,
    /// The sheet's unit normal: toward the first face of its pair.
    pub normal: [f64; 3],
    /// The sheet's plane frame: the halfway plane, in the coordinates `holes` are read in.
    pub frame: Plane,
    /// The holes through the sheet (an opening through both faces of the pair), in `frame`'s
    /// coordinates: round ones stay round.
    pub holes: Vec<Loop2>,
    /// What the analyst should know: what was left out.
    pub notes: Vec<String>,
}

/// Whether two points agree within `tol` in every coordinate (a comparison, no geometry).
fn near(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

/// The side of a polygon (`corners`, in order) that runs from `e0` to `e1`, either way, each end
/// within `tol`: its first corner's index (a comparison, no geometry).
fn edge_along(corners: &[[f64; 3]], e0: [f64; 3], e1: [f64; 3], tol: f64) -> Option<usize> {
    let count = corners.len();
    (0..count).find(|k| {
        let (Some(c0), Some(c1)) = (corners.get(*k).copied(), corners.get((*k + 1) % count).copied()) else { return false };
        (near(c0, e0, tol) && near(c1, e1, tol)) || (near(c0, e1, tol) && near(c1, e0, tol))
    })
}

/// What a caller may ask of the sheets beyond the pairs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Build {
    /// Carry holes through both faces into the sheets (off: every sheet is filled).
    pub carry_holes: bool,
    /// Holes no larger across than this (mm) are filled over; 0 keeps all.
    pub suppress_holes_below: f64,
}

impl Default for Build {
    fn default() -> Build {
        Build { carry_holes: true, suppress_holes_below: 0.0 }
    }
}

/// [`sheets_with`] with the default [`Build`]: holes carried, none suppressed.
pub fn sheets(body: &Body, pairs: &[Pair]) -> Result<Vec<Sheet>> {
    sheets_with(body, pairs, &Build::default())
}

/// The widest a hole is: the largest distance between two points of its outline, whatever way
/// it is turned (a slot at 45 degrees is as long as the same slot along an axis). Straight edges
/// are exact; arcs are read at a ten-thousandth of the hole's size.
fn hole_size(h: &Loop2, tol: f64) -> f64 {
    let rough = extent(&Region2 { outer: h.clone(), holes: Vec::new() }, tol).map_or(0.0, |(x, y)| (x[1] - x[0]).max(y[1] - y[0]));
    let pts = h.polyline((rough * 1e-4).max(tol));
    let pts: Vec<Vec2> = pts.iter().copied().collect();
    pts.iter().enumerate().flat_map(|(i, p)| pts.iter().skip(i + 1).map(move |q| p.dist(*q))).fold(0.0, f64::max)
}

/// Whether `build` would fill a hole of a round wall's outer face `outer_face`: one no wider than
/// its size to fill below, as [`hole_size`] fills a flat hole. A drilled hole's outline is a curve
/// no chart loop holds, so each loop of the face is measured on its exact edges (the largest
/// distance between two of their points, never more than the outline's length on the face): when
/// every loop is wider than the size, so is every hole, and none is filled. Otherwise, or when a
/// loop cannot be read, true: the build refuses rather than carry a hole it was asked to fill.
fn round_hole_filled(body: &Body, outer_face: usize, build: &Build) -> bool {
    let q = Query::new(body.record());
    let Some(face) = q.record().faces.get(outer_face) else { return true };
    let below = build.suppress_holes_below;
    face.loops.iter().any(|l| {
        let Bounds::Edges { edges } = l else { return true };
        let mut points: Vec<Vec3> = Vec::new();
        for u in edges {
            let e = u.edge as usize;
            let Some([t0, t1]) = q.edge_interval(e) else { return true };
            for k in 0..=8 {
                let Some((p, _)) = q.edge_at(e, t0 + (t1 - t0) * (f64::from(k) / 8.0)) else { return true };
                points.push(p);
            }
        }
        // Wider than `below` as soon as two points are farther apart.
        !points.iter().enumerate().any(|(i, a)| points.iter().skip(i + 1).any(|b| a.dist(*b) > below))
    })
}

/// The mid-sheets of `pairs` (a choice among an [`Analysis`]'s pairs), joined: where a solid edge
/// joins faces of two different pairs and lies along one of the sheets' edges, that edge is moved
/// onto the line the two halfway planes meet in (the sheet grows at an inside corner, and is
/// trimmed where it ran past).
pub fn sheets_with(body: &Body, pairs: &[Pair], build: &Build) -> Result<Vec<Sheet>> {
    let bad = |m: String| DocError::Invalid(format!("midsurface: {m}"));
    if body.is_mesh() {
        return Err(bad("a mesh body has no exact faces to join".into()));
    }
    if !(build.suppress_holes_below.is_finite() && build.suppress_holes_below >= 0.0) {
        return Err(bad("the size below which holes are filled must be a finite number, zero or more".into()));
    }
    let n = body.face_count();
    if let Some(p) = pairs.iter().find(|p| p.faces.iter().any(|f| *f >= n) || p.faces[0] == p.faces[1]) {
        return Err(bad(format!("the pair of faces {:?} is not a pair of this body's {n} faces", p.faces)));
    }
    let tol = (body.size() * 1e-6).max(1e-6);
    let matching = tol * 100.0;
    // One polygon per piece a pair shares.
    let mut built: Vec<(usize, Sheet)> = Vec::new();
    for (i, p) in pairs.iter().enumerate() {
        if let Some(round) = &p.round {
            // The faces cover different parts of the wall: the outer face's offset would add the
            // parts with no inner face. Not built (`report` names its faces), even when accepted.
            if !round.whole {
                continue;
            }
            // The offset carries every hole of the outer face: holes cannot be filled over yet. A
            // size to fill below that every hole is wider than fills none of them.
            if round.holes > 0 && (!build.carry_holes || (build.suppress_holes_below > 0.0 && round_hole_filled(body, round.outer_face, build))) {
                return Err(bad(format!(
                    "the round wall of faces {:?} has {} holes, and holes in a round wall cannot be filled over yet: build it with holes carried and none suppressed, or leave the pair out",
                    p.faces, round.holes
                )));
            }
            // The outer face of the round wall, taken halfway in: a cylinder stays a cylinder.
            let made = offset_surface(body, &[round.outer_face], -(p.thickness * 0.5), 1e-3)
                .map_err(|e| bad(format!("the round wall of faces {:?}: {e}", p.faces)))?;
            built.push((
                i,
                Sheet {
                    body: Some(made),
                    faces: p.faces,
                    thickness: p.thickness,
                    corners: Vec::new(),
                    outline: Loop2::default(),
                    curved: true,
                    collapsed: false,
                    normal: [0.0, 0.0, 0.0],
                    frame: Plane::XY,
                    holes: Vec::new(),
                    notes: vec!["a round wall: flat sheets meet it tangentially, with no extension".to_string()],
                },
            ));
            continue;
        }
        // The halfway plane has the first face's axes; its origin is the first face's dropped
        // onto it, so the holes' and the outline's coordinates are the same there.
        let origin = ms::onto_plane(&p.mid, array(p.frame.origin)).map_err(ops_err("the halfway plane"))?;
        let frame = Plane { origin: vec3(origin), x: p.frame.x, y: p.frame.y };
        let sheet_frame = frame_of(&frame);
        for region in &p.shared {
            let mut corners = Vec::new();
            let mut curved = false;
            for seg in &region.outer.segs {
                curved |= !matches!(seg, Seg2::Line { .. });
                let a = seg.start();
                corners.push(ms::from_frame(&sheet_frame, [a.x, a.y]).map_err(ops_err("a sheet's corner"))?);
            }
            let mut notes = Vec::new();
            if curved {
                notes.push("its outline has curved edges: its joints with other sheets are not extended or trimmed".to_string());
            }
            let mut holes = Vec::new();
            let mut dropped = 0;
            for h in &region.holes {
                // With no size asked for, every hole is kept without measuring it.
                if build.carry_holes && (build.suppress_holes_below <= 0.0 || hole_size(h, tol) > build.suppress_holes_below) {
                    holes.push(h.clone());
                } else {
                    dropped += 1;
                }
            }
            if dropped > 0 {
                notes.push(format!("{dropped} holes were filled over, as asked"));
            }
            if p.filled_over > 0.0 {
                notes.push(format!(
                    "filled over {:.1} mm² of openings that do not go straight through (in one face only, or closed or narrowing between the faces: a boss, rib, pocket or counterbore stands there): model that part separately",
                    p.filled_over
                ));
            }
            built.push((
                i,
                Sheet {
                    body: None,
                    faces: p.faces,
                    thickness: p.thickness,
                    corners,
                    normal: p.mid.outward,
                    frame,
                    outline: region.outer.clone(),
                    curved,
                    collapsed: false,
                    holes,
                    notes,
                },
            ));
        }
    }
    // The joints: solid edges between faces of two pairs. Each edge is matched against the
    // sheets' corners as they were before any joint moved them (at a corner where three walls
    // meet, the first joint moves the corner the second's edge ends at); the move is made on the
    // corners as they are now, at the same place (a move keeps the number of corners).
    let original: Vec<Vec<[f64; 3]>> = built.iter().map(|(_, s)| s.corners.clone()).collect();
    // For each pair of pairs that meet, whether each one's sheet was joined along some edge.
    let mut meets: Vec<(usize, usize, bool)> = Vec::new();
    // Flat pairs (indices) meeting a round wall (its faces) at an angle, not tangentially.
    let mut angled: Vec<(usize, [usize; 2])> = Vec::new();
    let q = Query::new(body.record());
    for e in 0..body.record().edges.len() {
        let on = q.edge_faces(e);
        let [fa, fb] = on.as_slice() else { continue };
        let ends = q.edge_interval(e).and_then(|[t0, t1]| Some((q.edge_at(e, t0)?.0, q.edge_at(e, t1)?.0)));
        // Whether flat pair `i`'s sheet, as it was before any joint, has an edge along this one
        // seen on its halfway plane.
        let runs_along = |i: usize| {
            let (Some(p), Some((p0, p1))) = (pairs.get(i), ends) else { return false };
            let (Ok(e0), Ok(e1)) = (ms::onto_plane(&p.mid, array(p0)), ms::onto_plane(&p.mid, array(p1))) else { return false };
            p.round.is_none()
                && built.iter().zip(&original).any(|((j, s), before)| *j == i && !s.curved && edge_along(before, e0, e1, matching).is_some())
        };
        // The pairs holding a face. A face in several (a skin split by the walls behind it): the
        // ones whose sheets run along the edge meet there, whatever the pairs' order; the others
        // cover another part of the face. When none does, each is tried, and says so.
        let owners = |f: &usize| -> Vec<usize> {
            let all: Vec<usize> = pairs.iter().enumerate().filter(|(_, p)| p.faces.contains(f)).map(|(i, _)| i).collect();
            if all.len() < 2 {
                return all;
            }
            let along: Vec<usize> = all.iter().copied().filter(|i| runs_along(*i)).collect();
            if along.is_empty() { all } else { along }
        };
        let (side_a, side_b) = (owners(fa), owners(fb));
        for (ia, ib) in side_a.iter().flat_map(|ia| side_b.iter().map(move |ib| (*ia, *ib))) {
            if ia == ib {
                continue;
            }
            let (Some(pa), Some(pb)) = (pairs.get(ia), pairs.get(ib)) else { continue };
            // A flat wall meeting a round one tangentially (a bend and its leg) needs no joint:
            // their sheets meet on the tangent line. One meeting it at an angle is not joined
            // yet: said.
            if pa.round.is_some() || pb.round.is_some() {
                if let (None, Some(_)) | (Some(_), None) = (&pa.round, &pb.round) {
                    let flat = if pa.round.is_none() { ia } else { ib };
                    // The kernel decides whether the two faces meet tangentially all along the edge.
                    let mid_point = q.edge_interval(e).and_then(|[t0, t1]| q.edge_at(e, 0.5 * (t0 + t1))).map(|(m, _)| m);
                    let tangent = mid_point.is_some_and(|m| smooth_picks(body, &[m]).is_ok_and(|s| s.first() == Some(&true)));
                    let round_faces = if flat == ia { pb.faces } else { pa.faces };
                    if !tangent && !angled.contains(&(flat, round_faces)) {
                        angled.push((flat, round_faces));
                    }
                }
                continue;
            }
            let joint = planes_meeting(&pa.mid, &pb.mid).map_err(|e| bad(format!("joint of pairs {ia} and {ib}: {e}")))?;
            let Some(joint) = joint else { continue };
            let Some((p0, p1)) = ends else { continue };
            for (owner_index, other, plane) in [(ia, ib, &pa.mid), (ib, ia, &pb.mid)] {
                let mut joined = false;
                // The edge seen on this pair's halfway plane.
                if let (Ok(e0), Ok(e1)) = (ms::onto_plane(plane, array(p0)), ms::onto_plane(plane, array(p1))) {
                    for ((_, sheet), before) in built.iter_mut().zip(&original).filter(|((i, s), _)| *i == owner_index && !s.curved) {
                        if before.len() != sheet.corners.len() {
                            continue;
                        }
                        let Some(k) = edge_along(before, e0, e1, matching) else { continue };
                        // A sheet its earlier joints already closed up (a rib as tall as the
                        // plate) has no edge left to move: it is absorbed below, and needs no
                        // more joints.
                        if sheet.corners != *before && ms::polygon_area(&sheet.corners).map_err(ops_err("a sheet's area"))? <= matching * matching {
                            joined = true;
                            continue;
                        }
                        let moved = move_edge_onto_line(&sheet.corners, k, &joint)
                            .map_err(|e| bad(format!("moving an edge of the sheet of faces {:?}: {e}", sheet.faces)))?;
                        if let Some(moved) = moved {
                            sheet.corners = moved;
                            joined = true;
                        }
                    }
                }
                match meets.iter_mut().find(|(o, w, _)| *o == owner_index && *w == other) {
                    Some(slot) => slot.2 |= joined,
                    None => meets.push((owner_index, other, joined)),
                }
            }
        }
    }
    // A wall no taller than the plate it stands in (a rib between slots) has both its edges moved
    // onto the plate's halfway line: nothing is left of it, and the plate already holds that
    // material. Only a sheet a joint moved: a small plate no joint touched keeps its sheet.
    for ((_, sheet), before) in built.iter_mut().zip(&original) {
        if !sheet.curved && sheet.corners != *before && ms::polygon_area(&sheet.corners).map_err(ops_err("a sheet's area"))? <= matching * matching {
            sheet.collapsed = true;
            sheet.notes.push("its edges met on the joint lines: it lies within the walls it joins and is absorbed into them".to_string());
        }
    }
    // Never silent: a flat wall meeting a round one at an angle keeps its own edge there.
    for (flat, round_faces) in angled {
        for (_, sheet) in built.iter_mut().filter(|(i, s)| *i == flat && !s.collapsed) {
            sheet.notes.push(format!(
                "a joint with the round wall of faces {round_faces:?} was not made: they meet at an angle, not tangentially, and a flat sheet is not joined to a round one yet"
            ));
        }
    }
    // Never silent: a wall that meets another but whose sheet was joined to it along no edge (a
    // wall absorbed into the other needs no joint of its own).
    let standing = |built: &[(usize, Sheet)], i: usize| built.iter().any(|(j, s)| *j == i && !s.curved && !s.collapsed);
    for (owner, other, joined) in meets {
        if joined || !standing(&built, owner) || !standing(&built, other) {
            continue;
        }
        let Some(theirs) = pairs.get(other).map(|p| p.faces) else { continue };
        if let Some((_, sheet)) = built.iter_mut().find(|(i, s)| *i == owner && !s.curved && !s.collapsed) {
            sheet.notes.push(format!(
                "a joint with the sheet of faces {theirs:?} was not made: no edge of this sheet lies along it, so the two sheets may not meet"
            ));
        }
    }
    Ok(built.into_iter().map(|(_, s)| s).collect())
}

/// The pairs that need no decision: what to build when nobody has chosen. The ones for review
/// (a chunky rib, a competing pair) wait for a person or an agent to accept them.
pub fn confident(analysis: &Analysis) -> Vec<Pair> {
    analysis.pairs.iter().filter(|p| p.confidence == Confidence::High).cloned().collect()
}

/// The face of `body` at `p` (within a hundred-thousandth of its size), decided by the kernel
/// on the exact faces; the nearest when `p` lies on an edge. `None` off the body.
pub fn face_at(body: &Body, p: [f64; 3]) -> Option<usize> {
    let tol = (body.size() * 1e-5).max(1e-5);
    (0..body.face_count())
        .filter_map(|f| body.point_lies_on_face(f, vec3(p), tol).ok().flatten().map(|on| ((on - vec3(p)).len(), f)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, f)| f)
}

/// The pairs of `analysis` whose faces hold `points` (one pair of points per pair, as a design
/// stores them): the pairs a feature built, found again without the heuristics. A pair is matched
/// by its faces whatever its confidence.
///
/// # Errors
/// A point on no face, or two faces that are no pair of the analysis (the design changed so that
/// they no longer face each other within the limits).
pub fn pairs_at(body: &Body, analysis: &Analysis, points: &[[[f64; 3]; 2]]) -> Result<Vec<Pair>> {
    let face_at = |p: [f64; 3]| face_at(body, p);
    let mut out: Vec<Pair> = Vec::new();
    for [a, b] in points {
        let (Some(fa), Some(fb)) = (face_at(*a), face_at(*b)) else {
            return Err(DocError::Invalid(format!("midsurface: no face of the body at {a:?} or {b:?}")));
        };
        let found = analysis.pairs.iter().find(|p| p.faces.contains(&fa) && p.faces.contains(&fb) && fa != fb);
        let Some(found) = found else {
            return Err(DocError::Invalid(format!(
                "midsurface: faces {fa} and {fb} (at {a:?} and {b:?}) no longer face each other within the thickness limits"
            )));
        };
        if !out.iter().any(|p| p.faces == found.faces) {
            out.push(found.clone());
        }
    }
    Ok(out)
}

/// Where every face of the body went: in a built pair, the rim of a built wall, or reported.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Completeness {
    /// How many faces the body has: `paired`, `rims` and `reported` together hold each once.
    pub faces: usize,
    /// Faces of the pairs built.
    pub paired: Vec<usize>,
    /// Faces that are the thin ends of built walls.
    pub rims: Vec<usize>,
    /// Every other face, with the reason it is not covered.
    pub reported: Vec<(usize, String)>,
}

impl Completeness {
    /// Whether every face is accounted for: paired, a rim, or reported (always true by
    /// construction; the count check guards it).
    pub fn accounts_for_every_face(&self) -> bool {
        self.paired.len() + self.rims.len() + self.reported.len() == self.faces
    }
}

/// Where each face of the analysed body went when `built` (pairs of `analysis`) are built.
pub fn completeness(analysis: &Analysis, built: &[Pair]) -> Completeness {
    let built_index = |i: &usize| analysis.pairs.get(*i).is_some_and(|p| built.iter().any(|b| b.faces == p.faces));
    // A rim is covered when every point of its edges lies in a built wall: a rim two walls share
    // (an L bracket's end) is not covered by one of them alone.
    let rim_covered = |l: &Leftover| !l.held_by.is_empty() && l.held_by.iter().all(|set| set.iter().any(built_index));
    let mut c = Completeness { faces: analysis.faces, ..Completeness::default() };
    for f in 0..analysis.faces {
        if built.iter().any(|p| p.faces.contains(&f)) {
            c.paired.push(f);
            continue;
        }
        if let Some(p) = analysis.pairs.iter().find(|p| p.faces.contains(&f)) {
            let why = if p.confidence == Confidence::Review {
                format!("in a pair that needs a decision and was not built (the pair of faces {:?})", p.faces)
            } else {
                format!("in a pair that was not chosen (the pair of faces {:?})", p.faces)
            };
            c.reported.push((f, why));
            continue;
        }
        let leftover = analysis.leftovers.iter().find(|l| l.face == f);
        match leftover.map(|l| &l.reason) {
            Some(Reason::Rim { .. }) if leftover.is_some_and(rim_covered) => c.rims.push(f),
            Some(Reason::Rim { pairs }) if pairs.iter().any(built_index) => {
                c.reported.push((f, "the rim of walls only some of which were built: part of it lies outside them".to_string()));
            }
            Some(Reason::Rim { .. }) => c.reported.push((f, "the rim of a wall that was not built".to_string())),
            Some(Reason::NotPlanar) => c.reported.push((f, "curved, neither a plane nor a cylinder: not covered".to_string())),
            Some(Reason::OutOfRange { gap }) => c.reported.push((f, format!("its facing face is past the thickness limit ({gap:.3} mm away)"))),
            Some(Reason::TooThin { gap }) => {
                c.reported.push((f, format!("its facing face is nearer than the thinnest wall asked for ({gap:.3} mm away)")));
            }
            Some(Reason::NoOverlap { gap }) => {
                c.reported.push((f, format!("its facing face is within the thickness limits but they do not overlap ({gap:.3} mm away)")));
            }
            Some(Reason::NoFacingFace) => c.reported.push((f, "nothing faces it across material".to_string())),
            None => c.reported.push((f, "not analysed".to_string())),
        }
    }
    c
}

/// A wall thickness found among the confident pairs, and how much sheet it gives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cluster {
    /// The thickness (mm): the smallest of the cluster's pairs.
    pub thickness: f64,
    /// The largest thickness in the cluster (mm).
    pub up_to: f64,
    /// The pairs' total overlap area (mm²).
    pub area: f64,
    /// How many pairs.
    pub pairs: usize,
    /// A thickness limit that takes this cluster and the thinner ones and no thicker one: half
    /// as much again as the cluster, but no further than the middle to the next thicker pair (the
    /// next cluster's, one that needs a decision, or a wall past the analysis's limit). That pair lies more than twice the
    /// analysis's slack past the cluster, so the scan (which takes a wall up to the limit plus
    /// that slack) still leaves it out.
    pub suggested_limit: f64,
}

/// The distinct wall thicknesses among `analysis`'s confident pairs, thinnest first. Thicknesses
/// within 1% of the cluster's first, or within twice the analysis's slack of its thickest, are
/// one cluster: no limit could tell them apart.
pub fn thickness_clusters(analysis: &Analysis) -> Vec<Cluster> {
    let slack = analysis.tolerance.max(0.0);
    let mut ts: Vec<(f64, f64)> = analysis.pairs.iter().filter(|p| p.confidence == Confidence::High).map(|p| (p.thickness, p.overlap_area)).collect();
    ts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<Cluster> = Vec::new();
    for (t, area) in ts {
        match out.last_mut() {
            Some(c) if t <= c.thickness * 1.01 + 1e-6 || t <= c.up_to + 2.0 * slack => {
                c.up_to = t;
                c.area += area;
                c.pairs += 1;
            }
            _ => out.push(Cluster { thickness: t, up_to: t, area, pairs: 1, suggested_limit: 0.0 }),
        }
    }
    for c in &mut out {
        let own = c.up_to * 1.5;
        // The next thicker pair of any confidence: the next cluster's, or one that needs a
        // decision (chunky, competing, blocked) and could become confident at a lower limit (its
        // rival left out). Only one a limit can tell apart from this cluster: past 1% of its first
        // and more than twice the slack past its thickest, so the middle plus the slack (what the
        // scan takes) stops short of it. A wall past the analysis's own limit is no pair, but a
        // larger limit would take it: the thinnest one it saw counts too.
        let next = analysis
            .pairs
            .iter()
            .map(|p| p.thickness)
            .chain(analysis.beyond.iter().copied())
            .filter(|t| *t > c.thickness * 1.01 + 1e-6 && *t > c.up_to + 2.0 * slack)
            .min_by(f64::total_cmp);
        c.suggested_limit = next.map_or(own, |n| own.min(0.5 * (c.up_to + n)));
    }
    out
}

/// The largest thickness [`suggested_max_thickness`] probes for: a fifth of the body's size.
/// Also the limit to fall back on when the probe finds no confident pair, so the two searches
/// look as far.
pub fn probe_max_thickness(body: &Body) -> f64 {
    (body.size() * 0.2).max(1e-3)
}

/// The limit to use when none is given: probe the body with a generous limit (a fifth of its
/// size, past which no wall is shell-like at the default aspect) and take the thickest confident
/// cluster's suggestion. `None` when the probe finds no confident pair.
pub fn suggested_max_thickness(body: &Body) -> Result<Option<f64>> {
    let probe = Options { max_thickness: probe_max_thickness(body), ..Options::default() };
    let a = analyze(body, &probe)?;
    Ok(thickness_clusters(&a).last().map(|c| c.suggested_limit))
}

/// What a result leaves out or leaves to the analyst, in words: faces it does not cover, pairs
/// that need a decision, sheets built though something lies between their faces, and sheets whose
/// joints were not made. Empty when nothing is missing.
pub fn report(analysis: &Analysis, sheets: &[Sheet]) -> Vec<String> {
    report_chosen(analysis, &[], sheets)
}

/// [`report`] of a build of `chosen` (pairs of `analysis`): a chosen pair that was not built (a
/// round wall whose faces cover different parts of it) is named with the reason, a pair
/// built though something lies between its faces keeps its `blocked` note (and one filled over a
/// bore that narrows between its faces its `stepped` note, one whose faces overlap on only part
/// of them its `partial` note, and one a face of which could not be tested exactly its
/// `unchecked` note), and only the pairs neither chosen nor
/// built are counted as needing a decision.
pub fn report_chosen(analysis: &Analysis, chosen: &[Pair], sheets: &[Sheet]) -> Vec<String> {
    let mut out = Vec::new();
    for p in chosen.iter().filter(|p| !sheets.iter().any(|s| s.faces == p.faces)) {
        let why: Vec<&str> = p.notes.iter().filter(|n| n.starts_with("partial") || n.starts_with("opening")).map(String::as_str).collect();
        let why = if why.is_empty() { "its faces do not cover the same part of the wall".to_string() } else { why.join("; ") };
        out.push(format!("the pair of faces {:?} was chosen but not built: {why}", p.faces));
    }
    let built: Vec<Pair> = analysis.pairs.iter().filter(|p| sheets.iter().any(|s| s.faces == p.faces)).cloned().collect();
    // Building a blocked pair builds it, but its sheet still spans what lies between its faces
    // (air beside a web, another wall): said for each one built, chosen or not (the pairs are
    // found from the sheets, so [`report`] says it too). So is a bore through both faces that
    // narrows between them (counterbores either side): the sheet fills it.
    let chosen_built = chosen.iter().filter(|p| sheets.iter().any(|s| s.faces == p.faces) && !built.iter().any(|b| b.faces == p.faces));
    for p in built.iter().chain(chosen_built) {
        for note in p.notes.iter().filter(|n| n.starts_with("blocked")) {
            out.push(format!("the sheet of faces {:?} was built though it is {note}", p.faces));
        }
        for note in p.notes.iter().filter(|n| n.starts_with("stepped")) {
            out.push(format!("the sheet of faces {:?} was built over a bore: {note}", p.faces));
        }
        // Faces that overlap on only part of the smaller one: the sheet covers that part alone,
        // and the rest of each face is in no sheet.
        for note in p.notes.iter().filter(|n| n.starts_with("partial")) {
            out.push(format!("the sheet of faces {:?} covers only part of them: {note}", p.faces));
        }
        // A face the exact tests could not read: nothing found between is no proof of nothing.
        for note in p.notes.iter().filter(|n| n.starts_with("unchecked")) {
            out.push(format!("the sheet of faces {:?} was built, but {note}", p.faces));
        }
    }
    let c = completeness(analysis, &built);
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (f, why) in &c.reported {
        // One line per kind of reason: the kind is the text before the face's own details in brackets.
        let kind = why.split(" (").next().unwrap_or(why).to_string();
        match groups.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, faces)) => faces.push(*f),
            None => groups.push((kind, vec![*f])),
        }
    }
    for (kind, faces) in groups {
        let what = if kind.starts_with("curved") { format!("{kind} (bends, bores, bosses: the surface has gaps there)") } else { kind };
        out.push(format!("{} faces {:?}: {what}", faces.len(), faces));
    }
    let review = analysis
        .pairs
        .iter()
        .filter(|p| p.confidence == Confidence::Review && !chosen.iter().any(|c| c.faces == p.faces) && !built.iter().any(|b| b.faces == p.faces))
        .count();
    if review > 0 {
        out.push(format!("{review} pairs need a decision (see their notes)"));
    }
    let absorbed = sheets.iter().filter(|s| s.collapsed).count();
    if absorbed > 0 {
        out.push(format!("{absorbed} short walls lie within the plate they join and were absorbed into it (no sheet of their own)"));
    }
    let loose = sheets.iter().filter(|s| s.curved && s.body.is_none()).count();
    if loose > 0 {
        out.push(format!("{loose} sheets have curved outlines: where they meet other sheets they are not extended or trimmed"));
    }
    for s in sheets.iter().filter(|s| !s.collapsed) {
        for note in s.notes.iter().filter(|n| n.starts_with("filled over")) {
            out.push(format!("the sheet of faces {:?} {note}", s.faces));
        }
        for note in s.notes.iter().filter(|n| n.starts_with("a joint")) {
            out.push(format!("the sheet of faces {:?}: {note}", s.faces));
        }
    }
    out
}

/// Fails when `sheets` hold no sheet to build (each pair chosen is one not built yet, such as a
/// partial round wall), with `notes` (the build's [`report_chosen`]) saying why, rather than only
/// that there is no sheet.
pub fn something_built(sheets: &[Sheet], notes: &[String]) -> Result<()> {
    if sheets.iter().any(|s| !s.collapsed) {
        return Ok(());
    }
    let why = if notes.is_empty() { "no sheet could be made".to_string() } else { notes.join("; ") };
    Err(DocError::Invalid(format!("Midsurface: nothing to build: {why}")))
}

/// One surface body for `sheets`: each sheet a planar patch at its own corners, sewn where they
/// share an edge. Sheets that meet nowhere stay separate faces of the one body; edges that more
/// than two sheets meet along are left open (a T junction), which an FEA pre-processor merges.
pub fn surface_body(sheets: &[Sheet]) -> Result<Body> {
    let mut bodies = surface_bodies(sheets, true)?;
    match (bodies.pop(), bodies.is_empty()) {
        (Some(b), true) => Ok(b),
        _ => Err(DocError::Invalid("midsurface: the sheets did not make one body".into())),
    }
}

/// A flat sheet's region with its holes inside its outline. A joint that trims a sheet where it
/// ran past the joint line can cut across a hole the pair carried: the kernel's exact boolean then
/// cuts the holes out of the outline (the sheet is notched there, or in pieces), rather than a
/// patch with a hole outside its outline. A region whose holes all lie inside is kept as it is.
fn holes_within(region: Region2, faces: [usize; 2]) -> Result<Vec<Region2>> {
    let slack = 1e-7;
    let what = |e| DocError::Invalid(format!("midsurface: the holes of the sheet of faces {faces:?}: {e}"));
    let outline = Region2 { outer: region.outer.ccw(), holes: Vec::new() };
    let holes = openings(&region);
    let outside: f64 =
        planar_regions_boolean(&holes, std::slice::from_ref(&outline), BoolOp::Cut, slack).map_err(what)?.iter().map(|r| r.area()).sum();
    if outside <= slack * slack {
        return Ok(vec![region]);
    }
    planar_regions_boolean(std::slice::from_ref(&outline), &holes, BoolOp::Cut, slack).map_err(what)
}

/// The surface bodies of `sheets`: one, sewn where they meet, when `sew`; otherwise a body per
/// sheet, joined to none. Sheets that close up when sewn (a sealed hollow box's six walls) give a
/// solid, as every sewing does (`surface.stitch`): the same faces, with the side they bound.
pub fn surface_bodies(sheets: &[Sheet], sew: bool) -> Result<Vec<Body>> {
    let bad = |m: String| DocError::Invalid(format!("midsurface: {m}"));
    let mut patches: Vec<Body> = Vec::with_capacity(sheets.len());
    for sheet in sheets.iter().filter(|s| !s.collapsed) {
        if let Some(made) = &sheet.body {
            patches.push(made.clone());
            continue;
        }
        // A polygon has three corners at least; a curved outline may be one arc (a full circle).
        if (sheet.curved && sheet.outline.segs.is_empty()) || (!sheet.curved && sheet.corners.len() < 3) {
            return Err(bad("a sheet needs an outline: three corners at least, or a curved edge".into()));
        }
        // The sheet's own frame: its corners and holes are read in it.
        let plane = sheet.frame;
        let frame = frame_of(&plane);
        let mut pts: Vec<Vec2> = Vec::with_capacity(sheet.corners.len());
        for c in &sheet.corners {
            let [u, v] = ms::in_frame(&frame, *c).map_err(ops_err("a sheet's corner"))?;
            pts.push(Vec2::new(u, v));
        }
        // Corners the joints brought together are one corner.
        pts.dedup_by(|a, b| a.dist(*b) <= 1e-7);
        if pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(f, l)| f.dist(*l) <= 1e-7) {
            pts.pop();
        }
        if !sheet.curved && pts.len() < 3 {
            return Err(bad(format!("the sheet of faces {:?} has fewer than three distinct corners", sheet.faces)));
        }
        let outer = if sheet.curved { sheet.outline.clone() } else { Loop2::polygon(&pts) };
        let region = Region2 { outer, holes: sheet.holes.clone() };
        // A curved outline is never moved by a joint: its holes stay where the pair put them.
        let regions = if sheet.curved || sheet.holes.is_empty() { vec![region] } else { holes_within(region, sheet.faces)? };
        for r in &regions {
            patches.push(patch_region(&plane, r).map_err(|e| bad(format!("sheet of faces {:?}: {e}", sheet.faces)))?);
        }
    }
    match patches.as_slice() {
        [] => Err(bad("no sheets to make a body of".into())),
        [_] => Ok(patches),
        _ if !sew => Ok(patches),
        many => {
            let refs: Vec<&Body> = many.iter().collect();
            Ok(vec![stitch(&refs, 0.0).map_err(|e| bad(format!("sewing the sheets: {e}")))?])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::{Loop2, Vec2};
    use solidcraft_kernel::extrude;
    use std::f64::consts::{FRAC_PI_2, PI};

    fn rect(w: f64, h: f64) -> Region2 {
        Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)]), holes: vec![] }
    }

    fn block(w: f64, h: f64, t: f64) -> Body {
        extrude(&Plane::XY, &[rect(w, h)], 0.0, t).unwrap().pop().unwrap()
    }

    fn opts(max: f64) -> Options {
        Options { min_thickness: 0.0, max_thickness: max, min_aspect: 5.0, tolerance: 0.0 }
    }

    fn rims(a: &Analysis) -> usize {
        a.leftovers.iter().filter(|l| matches!(l.reason, Reason::Rim { .. })).count()
    }

    #[test]
    fn a_plate_pairs_its_two_faces() {
        let a = analyze(&block(40.0, 30.0, 2.0), &opts(5.0)).unwrap();
        assert_eq!(a.pairs.len(), 1, "{a:?}");
        let p = &a.pairs[0];
        assert!((p.thickness - 2.0).abs() < 1e-6, "{p:?}");
        assert!((p.overlap_area - 1200.0).abs() < 1e-3, "{p:?}");
        assert!((p.coverage - 1.0).abs() < 1e-6);
        assert_eq!(p.confidence, Confidence::High, "{p:?}");
        assert_eq!(p.filled_over, 0.0);
        // The four end faces are 2 wide, as thick as the wall they end: rims, not gaps.
        assert_eq!(a.leftovers.len(), 4, "{a:?}");
        assert_eq!(rims(&a), 4, "{a:?}");
        // The pick points lie on the pair's own faces: z = 2 and z = 0, inside 40 x 30.
        for (pt, f) in p.points.iter().zip(p.faces) {
            assert!(pt[2].abs() < 1e-9 || (pt[2] - 2.0).abs() < 1e-9, "{pt:?}");
            assert!(pt[0] > 0.0 && pt[0] < 40.0 && pt[1] > 0.0 && pt[1] < 30.0, "{pt:?} of face {f}");
        }
        let c = completeness(&a, &confident(&a));
        assert_eq!((c.paired.len(), c.rims.len(), c.reported.len()), (2, 4, 0), "{c:?}");
        assert!(c.accounts_for_every_face());
        let built = sheets(&block(40.0, 30.0, 2.0), &a.pairs).unwrap();
        assert!(report(&a, &built).is_empty());
        // A pair built though a face could not be tested for blocking says so: the build is no
        // warning-free proof that nothing lies between its faces.
        let mut untested = a.clone();
        let note = "unchecked: face 7 could not be tested for blocking exactly".to_string();
        untested.pairs[0].notes.push(note.clone());
        for notes in [report(&untested, &built), report_chosen(&untested, &untested.pairs, &built)] {
            assert_eq!(notes.iter().filter(|n| n.ends_with(&note)).count(), 1, "{notes:?}");
        }
        // So does a pair built though its faces overlap on only part of the smaller one (two
        // skins shifted along each other): the rest of each face is in no sheet. The whole plate
        // above (coverage 1) has no such note and its report is empty.
        let mut shifted = a.clone();
        let note = "partial: the faces overlap on only 25% of the smaller one".to_string();
        shifted.pairs[0].notes.push(note.clone());
        for notes in [report(&shifted, &built), report_chosen(&shifted, &shifted.pairs, &built)] {
            assert_eq!(notes.iter().filter(|n| n.contains("covers only part") && n.ends_with(&note)).count(), 1, "{notes:?}");
        }
    }

    #[test]
    fn a_chunky_block_is_offered_for_review() {
        let a = analyze(&block(40.0, 30.0, 20.0), &opts(25.0)).unwrap();
        assert_eq!(a.pairs.len(), 1, "{a:?}");
        assert_eq!(a.pairs[0].confidence, Confidence::Review);
        assert!(a.pairs[0].notes.iter().any(|n| n.starts_with("chunky")), "{a:?}");
        // Not built, so its faces and rims are reported, each with a reason.
        let c = completeness(&a, &confident(&a));
        assert_eq!(c.reported.len(), 6, "{c:?}");
        assert!(c.accounts_for_every_face());
    }

    #[test]
    fn nothing_in_range_pairs_nothing() {
        let a = analyze(&block(40.0, 30.0, 20.0), &opts(5.0)).unwrap();
        assert!(a.pairs.is_empty());
        assert_eq!(a.leftovers.len(), 6);
        assert!(a.leftovers.iter().all(|l| matches!(l.reason, Reason::OutOfRange { .. })), "{a:?}");
    }

    /// The area of a planar polygon of 3D corners.
    fn area(corners: &[[f64; 3]]) -> f64 {
        let v = |c: [f64; 3]| Vec3::new(c[0], c[1], c[2]);
        let mut sum = Vec3::new(0.0, 0.0, 0.0);
        for i in 0..corners.len() {
            sum += v(corners[i]).cross(v(corners[(i + 1) % corners.len()]));
        }
        sum.len() * 0.5
    }

    fn l_bracket() -> Body {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0), Vec2::new(40.0, 2.0), Vec2::new(2.0, 2.0), Vec2::new(2.0, 30.0), Vec2::new(0.0, 30.0)];
        extrude(&Plane::XY, &[Region2 { outer: Loop2::polygon(&pts), holes: vec![] }], 0.0, 20.0).unwrap().pop().unwrap()
    }

    #[test]
    fn a_plates_sheet_is_its_faces_shared_area_halfway_between() {
        let a = analyze(&block(40.0, 30.0, 2.0), &opts(5.0)).unwrap();
        let s = sheets(&block(40.0, 30.0, 2.0), &a.pairs).unwrap();
        assert_eq!(s.len(), 1, "{s:?}");
        assert!((area(&s[0].corners) - 1200.0).abs() < 1e-6, "{s:?}");
        assert!(s[0].corners.iter().all(|c| (c[2] - 1.0).abs() < 1e-9), "{s:?}");
        assert!((s[0].thickness - 2.0).abs() < 1e-9);
    }

    #[test]
    fn an_l_brackets_sheets_meet_on_the_joint_and_total_the_closed_form() {
        let bracket = l_bracket();
        let a = analyze(&bracket, &opts(5.0)).unwrap();
        assert_eq!(a.pairs.len(), 2, "{a:?}");
        assert!(a.pairs.iter().all(|p| p.confidence == Confidence::High), "{a:?}");
        // Before joining: 38 x 20 and 28 x 20.
        let mut areas: Vec<f64> = a.pairs.iter().map(|p| p.overlap_area).collect();
        areas.sort_by(f64::total_cmp);
        assert!((areas[0] - 560.0).abs() < 1e-6 && (areas[1] - 760.0).abs() < 1e-6, "{areas:?}");
        // Joined: 39 x 20 and 29 x 20 = w (L1 + L2 - t) = 20 x 68 = 1360.
        let s = sheets(&bracket, &a.pairs).unwrap();
        assert_eq!(s.len(), 2, "{s:?}");
        let total: f64 = s.iter().map(|x| area(&x.corners)).sum();
        assert!((total - 1360.0).abs() < 1e-6, "{total} {s:?}");
        // Both sheets reach the line x = 1, y = 1.
        for sheet in &s {
            assert!(sheet.corners.iter().any(|c| (c[0] - 1.0).abs() < 1e-9 && (c[1] - 1.0).abs() < 1e-9), "{sheet:?}");
        }
    }

    #[test]
    fn an_l_brackets_rims_are_its_leg_ends_and_its_two_l_shaped_ends() {
        // 6 profile sides and 2 ends: 4 faces in the two pairs; the two leg tips (2 wide) and the
        // two L-shaped ends (2 wide all along) lie within the legs' walls.
        let bracket = l_bracket();
        let a = analyze(&bracket, &opts(5.0)).unwrap();
        assert_eq!(a.faces, 8);
        assert_eq!(rims(&a), 4, "{:?}", a.leftovers);
        let c = completeness(&a, &a.pairs);
        assert_eq!((c.paired.len(), c.rims.len(), c.reported.len()), (4, 4, 0), "{c:?}");
    }

    #[test]
    fn a_plates_sheet_becomes_a_surface_body_that_exports() {
        let plate = block(40.0, 30.0, 2.0);
        let a = analyze(&plate, &opts(5.0)).unwrap();
        let body = surface_body(&sheets(&plate, &a.pairs).unwrap()).unwrap();
        assert!(body.is_surface());
        let m = solidcraft_kernel::measure(&body).unwrap();
        assert!((m.area - 1200.0).abs() < 1e-6, "{m:?}");
        assert_eq!(m.faces, 1);
        let step = solidcraft_kernel::step_export(&[&body], "SolidCraft").unwrap();
        assert!(step.contains("FACE"), "{}", &step[..step.len().min(400)]);
    }

    #[test]
    fn a_small_plate_no_joint_moved_keeps_its_sheet_in_a_large_body() {
        // A strip 20000 x 20 x 0.2 and, apart from it, a plate 1 x 1 x 0.1 at (0..1, 100..101):
        // one body about 20000 across, so the matching distance is 100 x 20000e-6 = 2 mm and the
        // collapse cutoff 4 mm². The plate's 1 mm² sheet is under it, but no joint moved it (it
        // meets nothing): it is no rib absorbed into a wall and stays. With a 0.5 limit the two
        // plates are the only pairs (the rims face each other 1 and 20 mm apart): 400000 + 1 mm².
        let strip = block(20000.0, 20.0, 0.2);
        let square = Loop2::polygon(&[Vec2::new(0.0, 100.0), Vec2::new(1.0, 100.0), Vec2::new(1.0, 101.0), Vec2::new(0.0, 101.0)]);
        let small = extrude(&Plane::XY, &[Region2 { outer: square, holes: vec![] }], 0.0, 0.1).unwrap().pop().unwrap();
        let part = solidcraft_kernel::boolean(&strip, &small, BoolOp::Union).unwrap().unwrap();
        let a = analyze(&part, &opts(0.5)).unwrap();
        assert_eq!(a.pairs.len(), 2, "{:?}", a.pairs);
        let s = sheets(&part, &a.pairs).unwrap();
        let plate = s.iter().find(|x| (x.thickness - 0.1).abs() < 1e-9).unwrap_or_else(|| panic!("{s:?}"));
        assert!(!plate.collapsed, "{plate:?}");
        let total: f64 = surface_bodies(&s, false).unwrap().iter().map(|b| solidcraft_kernel::measure(b).unwrap().area).sum();
        assert!((total - 400_001.0).abs() < 1e-6, "{total}");
    }

    #[test]
    fn an_l_brackets_sheets_sew_into_one_surface_body() {
        let bracket = l_bracket();
        let a = analyze(&bracket, &opts(5.0)).unwrap();
        let s = sheets(&bracket, &a.pairs).unwrap();
        let body = surface_body(&s).unwrap();
        assert!(body.is_surface());
        let m = solidcraft_kernel::measure(&body).unwrap();
        assert!((m.area - 1360.0).abs() < 1e-6, "{m:?}");
        assert_eq!(m.faces, 2, "{m:?}");
        // Two sheets of four edges share one: seven edges, six on the boundary.
        assert_eq!(m.edges, 7, "{m:?}");
        assert_eq!(body.open_edge_count().unwrap(), 6);
        // Not sewn: a body per sheet, the same area.
        let apart = surface_bodies(&s, false).unwrap();
        assert_eq!(apart.len(), 2);
        let total: f64 = apart.iter().map(|b| solidcraft_kernel::measure(b).unwrap().area).sum();
        assert!((total - 1360.0).abs() < 1e-6, "{total}");
    }

    #[test]
    fn a_hole_through_a_plate_is_carried_to_its_surface_round() {
        let hole = Loop2::circle(Vec2::new(20.0, 15.0), 5.0);
        let plate = extrude(&Plane::XY, &[Region2 { outer: rect(40.0, 30.0).outer, holes: vec![hole] }], 0.0, 2.0).unwrap().pop().unwrap();
        let a = analyze(&plate, &opts(5.0)).unwrap();
        assert_eq!(a.pairs.len(), 1, "{a:?}");
        assert_eq!(a.pairs[0].confidence, Confidence::High, "{a:?}");
        let expected = 1200.0 - PI * 25.0;
        assert!((a.pairs[0].overlap_area - expected).abs() < 1e-6, "{:?}", a.pairs[0]);
        assert_eq!(a.pairs[0].filled_over, 0.0, "a hole through both faces is no one-sided opening");
        // The bore (2 tall) is a rim of the plate, as its four ends are.
        let c = completeness(&a, &a.pairs);
        assert!(c.reported.is_empty() && c.accounts_for_every_face(), "{c:?} {:?}", a.leftovers);
        let s = sheets(&plate, &a.pairs).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].holes.len(), 1);
        let body = surface_body(&s).unwrap();
        let m = solidcraft_kernel::measure(&body).unwrap();
        assert_eq!(m.faces, 1);
        assert!((m.area - expected).abs() < 0.05, "{} vs {expected}", m.area);
        let open = body.open_edge_count().unwrap();
        assert_eq!(open, m.edges);
        assert!(open >= 5, "{open}");
        // Holes up to 10 across filled over on request (the hole is 10 across): the full 1200.
        let filled = sheets_with(&plate, &a.pairs, &Build { carry_holes: true, suppress_holes_below: 10.5 }).unwrap();
        assert!(filled[0].holes.is_empty());
        let m = solidcraft_kernel::measure(&surface_body(&filled).unwrap()).unwrap();
        assert!((m.area - 1200.0).abs() < 1e-6, "{m:?}");
        let kept = sheets_with(&plate, &a.pairs, &Build { carry_holes: true, suppress_holes_below: 9.5 }).unwrap();
        assert_eq!(kept[0].holes.len(), 1);
    }

    #[test]
    fn a_sheet_trimmed_across_its_hole_is_notched_not_left_with_a_hole_outside_it() {
        // A plate 40 x 30 x 2 with a hole of radius 2 through it at (36, 15): one sheet carrying
        // the hole. Its outline's x = 40 side moved to x = 36, as a joint trimming the sheet
        // moves an edge, runs through the hole's centre: the sheet is 36 x 30 less the half of
        // the hole inside it, 1080 - 2 pi (the whole hole taken off would be 1080 - 4 pi).
        let hole = Loop2::circle(Vec2::new(36.0, 15.0), 2.0);
        let plate = extrude(&Plane::XY, &[Region2 { outer: rect(40.0, 30.0).outer, holes: vec![hole] }], 0.0, 2.0).unwrap().pop().unwrap();
        let a = analyze(&plate, &opts(5.0)).unwrap();
        let mut s = sheets(&plate, &a.pairs).unwrap();
        assert!(s.len() == 1 && s[0].holes.len() == 1, "{s:?}");
        for c in &mut s[0].corners {
            if (c[0] - 40.0).abs() < 1e-9 {
                c[0] = 36.0;
            }
        }
        let m = solidcraft_kernel::measure(&surface_body(&s).unwrap()).unwrap();
        let expected = 1080.0 - 2.0 * PI;
        assert!((m.area - expected).abs() < 0.05, "{} vs {expected}", m.area);
    }

    #[test]
    fn a_boss_on_a_plate_is_filled_over_and_reported_not_left_as_a_hole() {
        // A plate 40 x 30 x 2 and a boss of radius 5, 8 tall, on its top at (20, 15): the top face
        // has an opening where the boss stands, the bottom none. The sheet covers all 1200 mm²,
        // filled over the boss's foot of 25 pi.
        let plate = block(40.0, 30.0, 2.0);
        let boss =
            extrude(&Plane::XY, &[Region2 { outer: Loop2::circle(Vec2::new(20.0, 15.0), 5.0), holes: vec![] }], 2.0, 8.0).unwrap().pop().unwrap();
        let part = solidcraft_kernel::boolean(&plate, &boss, BoolOp::Union).unwrap().unwrap();
        let a = analyze(&part, &opts(5.0)).unwrap();
        let flat: Vec<&Pair> = a.pairs.iter().filter(|p| !p.is_round()).collect();
        assert_eq!(flat.len(), 1, "{:?}", a.pairs);
        assert!((flat[0].overlap_area - 1200.0).abs() < 1e-6, "{:?}", flat[0]);
        assert!((flat[0].filled_over - 25.0 * PI).abs() < 1e-3, "{:?}", flat[0]);
        let s = sheets(&part, &confident(&a)).unwrap();
        assert!(s.iter().all(|x| x.holes.is_empty()), "{s:?}");
        let notes = report(&a, &s);
        assert!(notes.iter().any(|n| n.contains("filled over")), "{notes:?}");
        // The boss's side and top are reported, not silently dropped.
        let c = completeness(&a, &confident(&a));
        assert!(c.accounts_for_every_face());
        assert!(c.reported.len() >= 2, "{c:?}");
    }

    #[test]
    fn a_round_plate_gives_a_round_sheet() {
        let disc =
            extrude(&Plane::XY, &[Region2 { outer: Loop2::circle(Vec2::new(0.0, 0.0), 20.0), holes: vec![] }], 0.0, 2.0).unwrap().pop().unwrap();
        let a = analyze(&disc, &opts(5.0)).unwrap();
        assert_eq!(a.pairs.len(), 1, "{a:?}");
        let s = sheets(&disc, &a.pairs).unwrap();
        assert_eq!(s.len(), 1);
        assert!(s[0].curved);
        let body = surface_body(&s).unwrap();
        let m = solidcraft_kernel::measure(&body).unwrap();
        let expected = PI * 400.0;
        assert!((m.area - expected).abs() < 0.05, "{} vs {expected}", m.area);
        // The round side, 2 tall, is the disc's rim: nothing is missing.
        let c = completeness(&a, &a.pairs);
        assert!(c.reported.is_empty(), "{c:?}");
    }

    #[test]
    fn a_tubes_wall_becomes_the_cylinder_halfway_between_its_faces() {
        // Outer radius 10, bore radius 8, 50 long: wall 2, mid radius 9, area 2 pi 9 50.
        let tube = extrude(
            &Plane::XY,
            &[Region2 { outer: Loop2::circle(Vec2::new(0.0, 0.0), 10.0), holes: vec![Loop2::circle(Vec2::new(0.0, 0.0), 8.0)] }],
            0.0,
            50.0,
        )
        .unwrap()
        .pop()
        .unwrap();
        let a = analyze(&tube, &opts(5.0)).unwrap();
        let round: Vec<&Pair> = a.pairs.iter().filter(|p| p.is_round()).collect();
        assert_eq!(round.len(), 1, "{a:?}");
        assert!((round[0].thickness - 2.0).abs() < 1e-9, "{:?}", round[0]);
        assert_eq!(round[0].confidence, Confidence::High, "{:?}", round[0]);
        let expected = 2.0 * PI * 9.0 * 50.0;
        assert!((round[0].overlap_area - expected).abs() < expected * 1e-3, "{} vs {expected}", round[0].overlap_area);
        // The pick points lie on radius 10 and 8.
        let radii: Vec<f64> = round[0].points.iter().map(|p| (p[0] * p[0] + p[1] * p[1]).sqrt()).collect();
        assert!(radii.iter().any(|r| (r - 10.0).abs() < 1e-6) && radii.iter().any(|r| (r - 8.0).abs() < 1e-6), "{radii:?}");
        let s = sheets(&tube, &confident(&a)).unwrap();
        assert_eq!(s.len(), 1);
        let body = surface_body(&s).unwrap();
        let m = solidcraft_kernel::measure(&body).unwrap();
        assert!((m.area - expected).abs() < expected * 2e-3, "{} vs {expected}", m.area);
        assert!(body.is_surface());
        // The two end rings lie between radii 8 and 10: rims.
        let c = completeness(&a, &confident(&a));
        assert!(c.reported.is_empty() && c.rims.len() == 2, "{c:?}");
    }

    #[test]
    fn a_quarter_round_bend_gives_its_mid_radius_sheet() {
        // A sheet bent a quarter turn: inside radius 3, outside 5, 20 wide; wall 2, mid radius 4.
        let section = Loop2 {
            segs: vec![
                Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 5.0, start: 0.0, sweep: FRAC_PI_2 },
                Seg2::Line { a: Vec2::new(0.0, 5.0), b: Vec2::new(0.0, 3.0) },
                Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 3.0, start: FRAC_PI_2, sweep: -FRAC_PI_2 },
                Seg2::Line { a: Vec2::new(3.0, 0.0), b: Vec2::new(5.0, 0.0) },
            ],
        };
        let bend = extrude(&Plane::XY, &[Region2 { outer: section, holes: vec![] }], 0.0, 20.0).unwrap().pop().unwrap();
        let a = analyze(&bend, &opts(5.0)).unwrap();
        let round: Vec<&Pair> = a.pairs.iter().filter(|p| p.is_round()).collect();
        assert_eq!(round.len(), 1, "{a:?}");
        assert_eq!(round[0].confidence, Confidence::High, "{:?}", round[0]);
        let expected = 4.0 * FRAC_PI_2 * 20.0;
        assert!((round[0].overlap_area - expected).abs() < expected * 1e-3, "{} vs {expected}", round[0].overlap_area);
        let s = sheets(&bend, &confident(&a)).unwrap();
        let body = surface_body(&s).unwrap();
        let m = solidcraft_kernel::measure(&body).unwrap();
        assert!((m.area - expected).abs() < expected * 2e-3, "{} vs {expected}", m.area);
        assert!(!report(&a, &s).iter().any(|n| n.contains("curved")), "{:?}", report(&a, &s));
    }

    /// A sheet 2 thick, 20 wide (along z): a flat leg 30 long on y = 0..2 (x -30..0), a quarter
    /// bend about (0, 5) of inside radius 3 and outside 5, and a flat leg 25 long on x = 3..5
    /// (y 5..30).
    fn leg_bend_leg() -> Body {
        let p = |x: f64, y: f64| Vec2::new(x, y);
        let section = Loop2 {
            segs: vec![
                Seg2::Line { a: p(-30.0, 0.0), b: p(0.0, 0.0) },
                Seg2::Arc { center: p(0.0, 5.0), radius: 5.0, start: -FRAC_PI_2, sweep: FRAC_PI_2 },
                Seg2::Line { a: p(5.0, 5.0), b: p(5.0, 30.0) },
                Seg2::Line { a: p(5.0, 30.0), b: p(3.0, 30.0) },
                Seg2::Line { a: p(3.0, 30.0), b: p(3.0, 5.0) },
                Seg2::Arc { center: p(0.0, 5.0), radius: 3.0, start: 0.0, sweep: -FRAC_PI_2 },
                Seg2::Line { a: p(0.0, 2.0), b: p(-30.0, 2.0) },
                Seg2::Line { a: p(-30.0, 2.0), b: p(-30.0, 0.0) },
            ],
        };
        extrude(&Plane::XY, &[Region2 { outer: section, holes: vec![] }], 0.0, 20.0).unwrap().pop().unwrap()
    }

    #[test]
    fn a_leg_a_bend_and_a_leg_give_their_developed_area_sewn_at_the_tangent_lines() {
        // Mid-surface: the legs' 30 and 25 at y = 1 and x = 4, and the bend's quarter of radius 4
        // (2 pi long), all 20 wide: 20 (30 + 25 + 2 pi) = 1100 + 40 pi. (The same as the
        // volume over the thickness: 20 (2 * 55 + pi (25 - 9) / 4) / 2.)
        let part = leg_bend_leg();
        let a = analyze(&part, &opts(5.0)).unwrap();
        assert_eq!(a.pairs.len(), 3, "{:?}", a.pairs);
        assert!(a.pairs.iter().all(|p| p.confidence == Confidence::High), "{:?}", a.pairs);
        let s = sheets(&part, &confident(&a)).unwrap();
        let body = surface_body(&s).unwrap();
        let m = solidcraft_kernel::measure(&body).unwrap();
        let expected = 1100.0 + 40.0 * PI;
        assert!((m.area - expected).abs() < expected * 1e-4, "{} vs {expected}", m.area);
        // Three sheets of four edges sewn along the two tangent lines: 10 edges, 8 open.
        assert_eq!(m.faces, 3, "{m:?}");
        assert_eq!(m.edges, 10, "{m:?}");
        assert_eq!(body.open_edge_count().unwrap(), 8);
        // The legs meet the bend tangentially: no joint is missing.
        assert!(!report(&a, &s).iter().any(|n| n.contains("a joint")), "{:?}", report(&a, &s));
        // 8 profile sides and 2 ends: 6 paired; the leg tips and the two ends are rims.
        let c = completeness(&a, &confident(&a));
        assert_eq!((c.faces, c.paired.len(), c.rims.len(), c.reported.len()), (10, 6, 4, 0), "{c:?}");
    }

    /// A plate 40 x 30 x 3 with two slots 12 x 20 (x 4..16 and 24..36, y 5..25).
    fn slotted_plate() -> Body {
        let slot = |x0: f64| Loop2::polygon(&[Vec2::new(x0, 5.0), Vec2::new(x0 + 12.0, 5.0), Vec2::new(x0 + 12.0, 25.0), Vec2::new(x0, 25.0)]);
        extrude(&Plane::XY, &[Region2 { outer: rect(40.0, 30.0).outer, holes: vec![slot(4.0), slot(24.0)] }], 0.0, 3.0).unwrap().pop().unwrap()
    }

    #[test]
    fn the_ribs_between_slots_are_offered_for_review_and_the_plate_is_built_without_them() {
        let plate = slotted_plate();
        let a = analyze(&plate, &opts(10.0)).unwrap();
        let chosen = confident(&a);
        assert_eq!(chosen.len(), 1, "{:?}", a.pairs.iter().map(|p| (p.faces, p.thickness, p.confidence)).collect::<Vec<_>>());
        assert!(a.pairs.len() > 1, "the ribs are candidates, for review");
        assert!(a.pairs.iter().filter(|p| p.confidence == Confidence::Review).all(|p| p.notes.iter().any(|n| n.starts_with("chunky"))));
        let s = sheets(&plate, &chosen).unwrap();
        let body = surface_body(&s).unwrap();
        let m = solidcraft_kernel::measure(&body).unwrap();
        assert!((m.area - 720.0).abs() < 1e-6, "{m:?}");
        let notes = report(&a, &s);
        assert!(notes.iter().any(|n| n.contains("need a decision")), "{notes:?}");
        // The ribs' faces are named with the whole reason, one line for all of them.
        assert_eq!(notes.iter().filter(|n| n.ends_with(": in a pair that needs a decision and was not built")).count(), 1, "{notes:?}");
    }

    #[test]
    fn ribs_as_tall_as_the_plate_collapse_when_built_and_never_make_an_empty_sheet() {
        let plate = slotted_plate();
        let a = analyze(&plate, &opts(10.0)).unwrap();
        // Everything accepted: the ribs' sheets have both edges moved onto the plate's halfway line.
        let s = sheets(&plate, &a.pairs).unwrap();
        assert!(s.iter().any(|x| x.collapsed), "{:?}", s.iter().map(|x| (x.faces, x.collapsed)).collect::<Vec<_>>());
        surface_body(&s).unwrap();
        assert!(report(&a, &s).iter().any(|n| n.contains("absorbed")));
    }

    /// An I beam 100 long: flanges 40 x 4, web 3 thick, 60 tall, the web centred at x = 20.
    fn i_beam() -> Body {
        let p = |x: f64, y: f64| Vec2::new(x, y);
        let pts = [
            p(0.0, 0.0),
            p(40.0, 0.0),
            p(40.0, 4.0),
            p(21.5, 4.0),
            p(21.5, 56.0),
            p(40.0, 56.0),
            p(40.0, 60.0),
            p(0.0, 60.0),
            p(0.0, 56.0),
            p(18.5, 56.0),
            p(18.5, 4.0),
            p(0.0, 4.0),
        ];
        extrude(&Plane::XY, &[Region2 { outer: Loop2::polygon(&pts), holes: vec![] }], 0.0, 100.0).unwrap().pop().unwrap()
    }

    #[test]
    fn an_i_beam_gives_a_web_and_two_flanges_each_in_two_halves() {
        let beam = i_beam();
        let a = analyze(&beam, &opts(5.0)).unwrap();
        // The web and four flange pieces; the flanges' outer faces are each in two pairs.
        assert_eq!(a.pairs.len(), 5, "{a:?}");
        assert!(a.pairs.iter().all(|p| p.confidence == Confidence::High), "{:?}", a.pairs);
        // 12 profile sides and 2 ends: 8 in pairs; the four flange tips (4 tall) and the two
        // I-shaped ends are rims.
        let c = completeness(&a, &a.pairs);
        assert_eq!((c.faces, c.paired.len(), c.rims.len(), c.reported.len()), (14, 8, 6, 0), "{c:?}");
        // Joined: flanges 2 x 40 x 100 and the web between their middles, 56 x 100.
        let s = sheets(&beam, &a.pairs).unwrap();
        assert_eq!(s.len(), 5, "{s:?}");
        let total: f64 = s.iter().map(|x| area(&x.corners)).sum();
        assert!((total - 13600.0).abs() < 1e-6, "{total}");
        let body = surface_body(&s).unwrap();
        let m = solidcraft_kernel::measure(&body).unwrap();
        assert!((m.area - 13600.0).abs() < 1e-6, "{m:?}");
        assert_eq!(m.faces, 5);
    }

    #[test]
    fn an_i_beams_two_thicknesses_are_two_clusters_with_a_limit_each() {
        // The web: 3 thick, 52 x 100 = 5200 mm². The flanges: 4 thick, four halves of 18.5 x 100,
        // 7400 mm². Limits: 3 x 1.5 = 4.5 is past the middle 3.5 between 3 and 4, so 3.5; and
        // 4 x 1.5 = 6.
        let a = analyze(&i_beam(), &opts(5.0)).unwrap();
        let c = thickness_clusters(&a);
        assert_eq!(c.len(), 2, "{c:?}");
        assert!((c[0].thickness - 3.0).abs() < 1e-9 && (c[0].area - 5200.0).abs() < 1e-6 && c[0].pairs == 1, "{c:?}");
        assert!((c[1].thickness - 4.0).abs() < 1e-9 && (c[1].area - 7400.0).abs() < 1e-6 && c[1].pairs == 4, "{c:?}");
        assert!((c[0].suggested_limit - 3.5).abs() < 1e-9 && (c[1].suggested_limit - 6.0).abs() < 1e-9, "{c:?}");
        // The beam is 40 x 60 x 100: a fifth of its size is past 4, so the probe sees both.
        let max = suggested_max_thickness(&i_beam()).unwrap();
        assert!(max.is_some_and(|m| (m - 6.0).abs() < 1e-9), "{max:?}");
    }

    #[test]
    fn a_suggested_limit_leaves_out_the_next_cluster_under_the_scans_slack() {
        // Slack 0.3: 4 is 1 past 3, more than twice 0.3, so two clusters; the limit 3.5 takes walls
        // up to 3.5 + 0.3 = 3.8: the web (3), not the flanges (4).
        let loose = |max: f64, tolerance: f64| Options { tolerance, ..opts(max) };
        let c = thickness_clusters(&analyze(&i_beam(), &loose(5.0, 0.3)).unwrap());
        assert!(c.len() == 2 && (c[0].suggested_limit - 3.5).abs() < 1e-9, "{c:?}");
        let web = analyze(&i_beam(), &loose(c[0].suggested_limit, 0.3)).unwrap();
        assert!(!web.pairs.is_empty() && web.pairs.iter().all(|p| (p.thickness - 3.0).abs() < 1e-9), "{:?}", web.pairs);
        // Slack 0.6: a limit of 3.5 would take walls up to 4.1, the flanges too. 4 lies within
        // twice 0.6 of 3, so no limit parts them under that slack: one cluster, limit 4 x 1.5 = 6.
        let c = thickness_clusters(&analyze(&i_beam(), &loose(5.0, 0.6)).unwrap());
        assert!(c.len() == 1 && (c[0].suggested_limit - 6.0).abs() < 1e-9 && c[0].pairs == 5, "{c:?}");
    }

    #[test]
    fn a_suggested_limit_leaves_out_a_thicker_pair_that_needs_a_decision() {
        // A plate 40 x 30 x 2: one confident pair, limit 2 x 1.5 = 3. Beside it a pair 2.5 thick
        // that needs a decision (a rival, say, that a lower limit leaves out, so it could become
        // confident): the limit stops at the middle, (2 + 2.5) / 2 = 2.25, and leaves it out. A
        // pair 2.01 thick (within 1% of 2) cannot be told apart by any limit and changes nothing.
        let mut a = analyze(&block(40.0, 30.0, 2.0), &opts(5.0)).unwrap();
        assert_eq!(a.pairs.len(), 1, "{:?}", a.pairs);
        let c = thickness_clusters(&a);
        assert!(c.len() == 1 && (c[0].suggested_limit - 3.0).abs() < 1e-9, "{c:?}");
        let plate = a.pairs[0].clone();
        let review = |t: f64| Pair { thickness: t, confidence: Confidence::Review, ..plate.clone() };
        a.pairs.push(review(2.01));
        let c = thickness_clusters(&a);
        assert!(c.len() == 1 && (c[0].suggested_limit - 3.0).abs() < 1e-9, "{c:?}");
        a.pairs.push(review(2.5));
        let c = thickness_clusters(&a);
        assert!(c.len() == 1 && c[0].pairs == 1 && (c[0].suggested_limit - 2.25).abs() < 1e-9, "{c:?}");
    }

    #[test]
    fn a_suggested_limit_leaves_out_a_wall_past_the_analysis_limit() {
        // Two plates 40 x 30 apart along x, 2 and 2.5 thick, analysed up to 2.1: the 2.5 wall is
        // no pair, but a limit of 2 x 1.5 = 3 would take it. The analysis saw it 2.5 thick, so
        // the limit stops at the middle, (2 + 2.5) / 2 = 2.25, and analysed with it the 2 mm
        // plate is the only pair.
        let part = union(&cuboid([0.0, 0.0, 0.0], [40.0, 30.0, 2.0]), &cuboid([100.0, 0.0, 0.0], [140.0, 30.0, 2.5]));
        let a = analyze(&part, &opts(2.1)).unwrap();
        assert!(a.pairs.len() == 1 && (a.pairs[0].thickness - 2.0).abs() < 1e-9, "{:?}", a.pairs);
        // 2.5 is past 2.1 by more than 1% and twice the slack: the 30 and 40 mm walls (the
        // plates' sides) lie past it and are not kept.
        assert!(a.beyond.len() == 1 && (a.beyond[0] - 2.5).abs() < 1e-9, "{:?}", a.beyond);
        let c = thickness_clusters(&a);
        assert!(c.len() == 1 && (c[0].suggested_limit - 2.25).abs() < 1e-9, "{c:?}");
        let again = analyze(&part, &opts(c[0].suggested_limit)).unwrap();
        assert!(again.pairs.len() == 1 && (again.pairs[0].thickness - 2.0).abs() < 1e-9, "{:?}", again.pairs);
    }

    #[test]
    fn a_suggested_limit_looks_past_a_wall_no_limit_tells_apart_from_its_cluster() {
        // Three plates 40 x 30, 2, 2.01 and 2.5 thick, analysed up to 2.005 with a slack of
        // 0.001: the 2 mm plate is the only pair; 2.01 and 2.5 lie past the limit. 2.01 is within
        // 1% of 2 (2.02), so no limit parts it from the cluster: the limit stops at the middle to
        // the next one, (2 + 2.5) / 2 = 2.25, not at 2 x 1.5 = 3, which would take the 2.5 wall.
        let plates = union(
            &union(&cuboid([0.0, 0.0, 0.0], [40.0, 30.0, 2.0]), &cuboid([100.0, 0.0, 0.0], [140.0, 30.0, 2.01])),
            &cuboid([200.0, 0.0, 0.0], [240.0, 30.0, 2.5]),
        );
        let scan = |max: f64| Options { tolerance: 0.001, ..opts(max) };
        let a = analyze(&plates, &scan(2.005)).unwrap();
        assert!(a.pairs.len() == 1 && (a.pairs[0].thickness - 2.0).abs() < 1e-9, "{:?}", a.pairs);
        // Kept: 2.01 (a cluster up to 2.006 could still take it) and the first wall past
        // 2.006 x 1.01 + 0.002 = 2.0281, the 2.5.
        assert!(a.beyond.len() == 2 && (a.beyond[0] - 2.01).abs() < 1e-9 && (a.beyond[1] - 2.5).abs() < 1e-9, "{:?}", a.beyond);
        let c = thickness_clusters(&a);
        assert!(c.len() == 1 && (c[0].suggested_limit - 2.25).abs() < 1e-9, "{c:?}");
        // Scanned with it (walls up to 2.251), the 2.5 plate is still left out.
        let again = analyze(&plates, &scan(c[0].suggested_limit)).unwrap();
        assert!(!again.pairs.is_empty() && again.pairs.iter().all(|p| p.thickness < 2.1), "{:?}", again.pairs);
    }

    #[test]
    fn stored_points_find_the_same_pairs_again() {
        let beam = i_beam();
        let a = analyze(&beam, &opts(5.0)).unwrap();
        let points: Vec<[[f64; 3]; 2]> = a.pairs.iter().map(|p| p.points).collect();
        let again = pairs_at(&beam, &a, &points).unwrap();
        assert_eq!(again.iter().map(|p| p.faces).collect::<Vec<_>>(), a.pairs.iter().map(|p| p.faces).collect::<Vec<_>>());
        // A point off the body, and two faces that are no pair, are refused.
        assert!(pairs_at(&beam, &a, &[[[500.0, 0.0, 0.0], [0.0, 0.0, 0.0]]]).is_err());
        let not_a_pair = [a.pairs[0].points[0], [0.0, 30.0, 50.0]];
        assert!(pairs_at(&beam, &a, &[not_a_pair]).is_err());
    }

    /// The pairs' thicknesses and areas, sorted: what a body's analysis says whatever its faces'
    /// order.
    fn walls(a: &Analysis) -> Vec<(f64, f64, Confidence)> {
        let mut w: Vec<(f64, f64, Confidence)> = a.pairs.iter().map(|p| (p.thickness, p.overlap_area, p.confidence)).collect();
        w.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)));
        w
    }

    #[test]
    fn a_solid_read_back_from_step_gives_the_same_pairs_and_area() {
        // The L bracket's joined sheets are 1360 mm² and the I beam's 13600 mm² (tests above).
        for (solid, expected) in [(l_bracket(), 1360.0), (i_beam(), 13600.0)] {
            let step = solidcraft_kernel::step_export(&[&solid], "SolidCraft").unwrap();
            let read = solidcraft_kernel::step_import(&step).unwrap();
            let back = &read.bodies[0].body;
            assert!(!back.is_mesh());
            let (a, b) = (analyze(&solid, &opts(5.0)).unwrap(), analyze(back, &opts(5.0)).unwrap());
            let (wa, wb) = (walls(&a), walls(&b));
            assert_eq!(wa.len(), wb.len(), "{wa:?} against {wb:?}");
            for (x, y) in wa.iter().zip(&wb) {
                assert!((x.0 - y.0).abs() < 1e-6 && (x.1 - y.1).abs() < 1e-6 && x.2 == y.2, "{wa:?} against {wb:?}");
            }
            let c = completeness(&b, &confident(&b));
            assert!(c.reported.is_empty(), "{c:?}");
            let m = solidcraft_kernel::measure(&surface_body(&sheets(back, &confident(&b)).unwrap()).unwrap()).unwrap();
            assert!((m.area - expected).abs() < 1e-6, "{} against {expected}", m.area);
        }
    }

    #[test]
    fn the_conformance_plate_with_100_holes_gives_one_sheet_carrying_them_all() {
        // tests/step-conformance/plate_100_holes.step: 100 x 100 x 10, its four vertical corners
        // rounded to radius 4 (its 8 circles of radius 4) and 100 holes of radius 3 (its 200
        // circles of radius 3); the importer's test reads its volume as 71588.321 = 10 x this area.
        // The sheet: 10000 - 4 (16 - 4 pi) - 100 (9 pi) = 9936 - 884 pi mm², with 100 holes.
        let read = solidcraft_kernel::step_import(include_str!("../../../tests/step-conformance/plate_100_holes.step")).unwrap();
        let plate = &read.bodies[0].body;
        assert!(!plate.is_mesh());
        assert_eq!(plate.face_count(), 110);
        let a = analyze(plate, &opts(20.0)).unwrap();
        assert_eq!(a.pairs.len(), 1, "{:?}", a.pairs);
        assert_eq!(a.pairs[0].confidence, Confidence::High, "{:?}", a.pairs[0]);
        assert!((a.pairs[0].thickness - 10.0).abs() < 1e-6);
        // The four sides, four corners and 100 bores are 10 tall, as thick as the wall: rims.
        let c = completeness(&a, &confident(&a));
        assert_eq!((c.paired.len(), c.rims.len(), c.reported.len()), (2, 108, 0), "{c:?}");
        let s = sheets(plate, &confident(&a)).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].holes.len(), 100);
        let m = solidcraft_kernel::measure(&surface_body(&s).unwrap()).unwrap();
        let expected = 9936.0 - 884.0 * PI;
        assert!((m.area - expected).abs() < expected * 1e-5, "{} against {expected}", m.area);
        assert_eq!(m.faces, 1);
    }

    /// A box from (x0, y0, z0) to (x1, y1, z1).
    fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Body {
        let p = |x: f64, y: f64| Vec2::new(x, y);
        let outer = Loop2::polygon(&[p(lo[0], lo[1]), p(hi[0], lo[1]), p(hi[0], hi[1]), p(lo[0], hi[1])]);
        extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], lo[2], hi[2] - lo[2]).unwrap().pop().unwrap()
    }

    fn union(a: &Body, b: &Body) -> Body {
        solidcraft_kernel::boolean(a, b, BoolOp::Union).unwrap().unwrap()
    }

    #[test]
    fn three_walls_meeting_at_a_corner_are_each_joined_to_both_others() {
        // A floor 40 x 40 x 2 and two walls 2 thick, 30 tall, along its x = 0 and y = 0 edges.
        // Before joining: the floor 38 x 38, each wall 38 x 28. Each sheet meets the other two;
        // the second joint of each starts at the corner the first moved. Joined, every sheet
        // reaches the halfway lines x = 1, y = 1, z = 1: the floor 39 x 39 = 1521 and each wall
        // 39 x 29 = 1131, 3783 in all.
        let corner = union(
            &union(&cuboid([0.0, 0.0, 0.0], [40.0, 40.0, 2.0]), &cuboid([0.0, 0.0, 0.0], [2.0, 40.0, 30.0])),
            &cuboid([0.0, 0.0, 0.0], [40.0, 2.0, 30.0]),
        );
        let a = analyze(&corner, &opts(5.0)).unwrap();
        assert_eq!(a.pairs.len(), 3, "{:?}", a.pairs);
        for pairs in [a.pairs.clone(), a.pairs.iter().rev().cloned().collect()] {
            let s = sheets(&corner, &pairs).unwrap();
            let mut areas: Vec<f64> = s.iter().map(|x| area(&x.corners)).collect();
            areas.sort_by(f64::total_cmp);
            assert!((areas[0] - 1131.0).abs() < 1e-6 && (areas[1] - 1131.0).abs() < 1e-6 && (areas[2] - 1521.0).abs() < 1e-6, "{areas:?}");
            assert!(!report(&a, &s).iter().any(|n| n.contains("a joint")), "{:?}", report(&a, &s));
        }
    }

    #[test]
    fn round_faces_of_the_same_sweep_at_different_angles_are_not_a_whole_wall() {
        // Outside radius 10 over 0 to 90 degrees, inside radius 8 over 45 to 135, 20 long: the
        // same length, the same sweep and the same chart area, but only 45 to 90 degrees has
        // material between the two. Offsetting the whole outer face would add 0 to 45 degrees.
        let c = std::f64::consts::FRAC_1_SQRT_2;
        let section = Loop2 {
            segs: vec![
                Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 10.0, start: 0.0, sweep: FRAC_PI_2 },
                Seg2::Line { a: Vec2::new(0.0, 10.0), b: Vec2::new(-8.0 * c, 8.0 * c) },
                Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 8.0, start: 3.0 * PI / 4.0, sweep: -FRAC_PI_2 },
                Seg2::Line { a: Vec2::new(8.0 * c, 8.0 * c), b: Vec2::new(10.0, 0.0) },
            ],
        };
        let part = extrude(&Plane::XY, &[Region2 { outer: section, holes: vec![] }], 0.0, 20.0).unwrap().pop().unwrap();
        let a = analyze(&part, &opts(5.0)).unwrap();
        let round: Vec<&Pair> = a.pairs.iter().filter(|p| p.is_round()).collect();
        assert_eq!(round.len(), 1, "{:?}", a.pairs);
        assert_eq!(round[0].confidence, Confidence::Review, "{:?}", round[0]);
        assert!(round[0].notes.iter().any(|n| n.starts_with("partial")), "{:?}", round[0]);
        // Its area is what the two share: 45 to 90 degrees (pi / 4) over the 20, at the mid
        // radius 9: 9 x 20 x pi / 4 = 45 pi, not the first face's whole quarter (90 pi).
        assert!((round[0].overlap_area - 45.0 * PI).abs() < 1e-6, "{:?}", round[0]);
        // Accepted anyway, it is not built, and its faces are reported.
        let s = sheets(&part, &a.pairs).unwrap();
        assert!(!s.iter().any(|x| x.faces == round[0].faces), "{s:?}");
        let c = completeness(&a, &s.iter().filter_map(|x| a.pairs.iter().find(|p| p.faces == x.faces).cloned()).collect::<Vec<_>>());
        assert!(round[0].faces.iter().all(|f| c.reported.iter().any(|(g, _)| g == f)), "{c:?}");
        // Chosen, the report names it and why, and does not ask for a decision already made.
        let notes = report_chosen(&a, &a.pairs, &s);
        let named = format!("the pair of faces {:?} was chosen but not built: partial", round[0].faces);
        assert!(notes.iter().any(|n| n.starts_with(&named)), "{notes:?}");
        assert!(!notes.iter().any(|n| n.contains("need a decision")), "{notes:?}");
        // Chosen alone, nothing is built: the build fails, and says why.
        let alone = std::slice::from_ref(round[0]);
        let s = sheets(&part, alone).unwrap();
        let error = something_built(&s, &report_chosen(&a, alone, &s)).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains(&named), "{error}");
    }

    #[test]
    fn a_wall_meeting_a_skin_two_pairs_share_is_joined_whatever_the_pairs_order() {
        // A plate 40 long, y 0..2, with a wall 2 thick up from its right end (x 38..40, y 0..30)
        // and a rib 2 thick down from its middle (x 19..21, y -10..0), all 20 deep. The top face
        // (y = 2, x 0..38) is in two pairs: with the bottom left of the rib (x 0..19) and right of
        // it (x 21..40). Only the right one's sheet (x 21..38) reaches the wall, along the edge
        // where the top meets the wall's inner face. Joined, each sheet reaches the halfway lines
        // x = 39, x = 20 and y = 1: left 20 x 20 = 400, right 19 x 20 = 380, wall 29 x 20 = 580,
        // rib 11 x 20 = 220; 1580 in all, whichever of the two pairs comes first.
        let p = |x: f64, y: f64| Vec2::new(x, y);
        let pts = [
            p(0.0, 0.0),
            p(19.0, 0.0),
            p(19.0, -10.0),
            p(21.0, -10.0),
            p(21.0, 0.0),
            p(40.0, 0.0),
            p(40.0, 30.0),
            p(38.0, 30.0),
            p(38.0, 2.0),
            p(0.0, 2.0),
        ];
        let part = extrude(&Plane::XY, &[Region2 { outer: Loop2::polygon(&pts), holes: vec![] }], 0.0, 20.0).unwrap().pop().unwrap();
        let a = analyze(&part, &opts(5.0)).unwrap();
        assert_eq!(a.pairs.len(), 4, "{:?}", a.pairs);
        assert!(a.pairs.iter().all(|p| p.confidence == Confidence::High), "{:?}", a.pairs);
        for pairs in [a.pairs.clone(), a.pairs.iter().rev().cloned().collect()] {
            let s = sheets(&part, &pairs).unwrap();
            let mut areas: Vec<f64> = s.iter().map(|x| area(&x.corners)).collect();
            areas.sort_by(f64::total_cmp);
            let expected = [220.0, 380.0, 400.0, 580.0];
            assert!(areas.len() == 4 && areas.iter().zip(expected).all(|(x, y)| (x - y).abs() < 1e-6), "{areas:?}");
            assert!(!report(&a, &s).iter().any(|n| n.contains("a joint")), "{:?}", report(&a, &s));
        }
    }

    #[test]
    fn a_round_wall_with_holes_is_refused_when_its_holes_were_asked_to_be_filled() {
        // The tube of radii 10 and 8, 50 long, drilled across at z = 25 (radius 1) through both
        // sides: the outer face has holes, which the offset carries; it cannot fill them. (The
        // holes take about pi/10 and pi/8 of the two faces' charts of 2 pi x 50 each, 0.05%
        // apart, within the 0.1% a whole wall allows; a wider drill makes the wall partial.)
        let tube = extrude(
            &Plane::XY,
            &[Region2 { outer: Loop2::circle(Vec2::new(0.0, 0.0), 10.0), holes: vec![Loop2::circle(Vec2::new(0.0, 0.0), 8.0)] }],
            0.0,
            50.0,
        )
        .unwrap()
        .pop()
        .unwrap();
        // Along y (the front plane's sketch y is -z), clear of the faces' seam at +x.
        let drill =
            extrude(&Plane::XZ, &[Region2 { outer: Loop2::circle(Vec2::new(0.0, -25.0), 1.0), holes: vec![] }], -15.0, 30.0).unwrap().pop().unwrap();
        let part = solidcraft_kernel::boolean(&tube, &drill, BoolOp::Cut).unwrap().unwrap();
        let a = analyze(&part, &opts(5.0)).unwrap();
        let round: Vec<Pair> = a.pairs.iter().filter(|p| p.is_round()).cloned().collect();
        assert!(!round.is_empty() && round.iter().all(|p| p.confidence == Confidence::High), "{:?}", a.pairs);
        assert!(sheets_with(&part, &round, &Build { carry_holes: false, suppress_holes_below: 0.0 }).is_err(), "{round:?}");
        assert!(sheets_with(&part, &round, &Build { carry_holes: true, suppress_holes_below: 5.0 }).is_err());
        assert!(sheets_with(&part, &round, &Build::default()).is_ok());
        // Each hole is about 2 wide on the outer face (the drill's diameter, along the axis and
        // round it): a size to fill below of 0.01 fills none of them, so the wall is built as
        // with none asked for; 2.5 would fill them, and is refused.
        assert!(sheets_with(&part, &round, &Build { carry_holes: true, suppress_holes_below: 0.01 }).is_ok());
        assert!(sheets_with(&part, &round, &Build { carry_holes: true, suppress_holes_below: 2.5 }).is_err());
    }

    #[test]
    fn two_blind_pockets_facing_each_other_are_filled_over_not_cut_through() {
        // A plate 40 x 30 x 2 with pockets of radius 5, 0.5 deep, at (20, 15) in its top and its
        // bottom: 1 mm of material between their floors. The two faces' openings coincide, but
        // nothing goes through: the sheet is the whole 1200 mm², filled over the 25 pi of mouth.
        let plate = block(40.0, 30.0, 2.0);
        let pocket = |z: f64| {
            extrude(&Plane::XY, &[Region2 { outer: Loop2::circle(Vec2::new(20.0, 15.0), 5.0), holes: vec![] }], z, z + 0.5).unwrap().pop().unwrap()
        };
        let cut = |b: &Body, t: &Body| solidcraft_kernel::boolean(b, t, BoolOp::Cut).unwrap().unwrap();
        let part = cut(&cut(&plate, &pocket(1.5)), &pocket(0.0));
        let a = analyze(&part, &opts(5.0)).unwrap();
        let main = a.pairs.iter().find(|p| (p.thickness - 2.0).abs() < 1e-9).unwrap_or_else(|| panic!("{:?}", a.pairs));
        assert!((main.overlap_area - 1200.0).abs() < 1e-6, "{main:?}");
        assert!((main.filled_over - 25.0 * PI).abs() < 1e-3, "{main:?}");
        let s = sheets(&part, std::slice::from_ref(main)).unwrap();
        assert!(s.iter().all(|x| x.holes.is_empty()), "{s:?}");
        // A hole through the plate is still carried (a_hole_through_a_plate_... above).
    }

    #[test]
    fn a_web_between_two_skins_is_seen_wherever_the_ray_falls() {
        // The I beam's two outer flange faces, 60 apart, with the limits round 60 and no aspect
        // limit: between them is the web under one strip and air beside it. One ray down the
        // web's middle meets nothing; the inner flange faces' and the web's edges lie between.
        let beam = i_beam();
        let a = analyze(&beam, &Options { min_thickness: 50.0, max_thickness: 65.0, min_aspect: 0.0, tolerance: 0.0 }).unwrap();
        assert_eq!(a.pairs.len(), 1, "{:?}", a.pairs);
        assert_eq!(a.pairs[0].confidence, Confidence::Review, "{:?}", a.pairs[0]);
        assert!(a.pairs[0].notes.iter().any(|n| n.starts_with("blocked")), "{:?}", a.pairs[0]);
        // Chosen anyway, it is built, and the report still says that its sheet spans air.
        let s = sheets(&beam, &a.pairs).unwrap();
        assert!(s.iter().any(|x| x.faces == a.pairs[0].faces), "{s:?}");
        let notes = report_chosen(&a, &a.pairs, &s);
        assert!(notes.iter().any(|n| n.contains("was built though it is blocked")), "{notes:?}");
        // The plain report finds the pair from its sheet and says so too.
        let notes = report(&a, &s);
        assert!(notes.iter().any(|n| n.contains("was built though it is blocked")), "{notes:?}");
    }

    #[test]
    fn a_slots_width_does_not_depend_on_which_way_it_is_turned() {
        // A slot 6 long and 1 wide at 45 degrees through a plate: 6.08 across corner to corner
        // (the square root of 37), whichever way it is turned; its box along the axes is only
        // 4.95 on each side. Holes up to 5.5 filled: it is kept; up to 6.5: filled.
        let c = std::f64::consts::FRAC_1_SQRT_2;
        let (u, v) = (Vec2::new(c, c), Vec2::new(-c, c));
        let at = |s: f64, t: f64| Vec2::new(20.0 + u.x * s + v.x * t, 15.0 + u.y * s + v.y * t);
        let slot = Loop2::polygon(&[at(-3.0, -0.5), at(3.0, -0.5), at(3.0, 0.5), at(-3.0, 0.5)]);
        let plate = extrude(&Plane::XY, &[Region2 { outer: rect(40.0, 30.0).outer, holes: vec![slot] }], 0.0, 2.0).unwrap().pop().unwrap();
        let a = analyze(&plate, &opts(5.0)).unwrap();
        let kept = sheets_with(&plate, &confident(&a), &Build { carry_holes: true, suppress_holes_below: 5.5 }).unwrap();
        assert_eq!(kept.iter().map(|s| s.holes.len()).sum::<usize>(), 1, "{kept:?}");
        let filled = sheets_with(&plate, &confident(&a), &Build { carry_holes: true, suppress_holes_below: 6.5 }).unwrap();
        assert_eq!(filled.iter().map(|s| s.holes.len()).sum::<usize>(), 0, "{filled:?}");
    }

    #[test]
    fn a_face_too_near_its_partner_says_so_not_that_it_is_too_far() {
        // A 2 mm plate with the limits 3 to 5: its big faces are 2 apart, below the thinnest.
        let a = analyze(&block(40.0, 30.0, 2.0), &Options { min_thickness: 3.0, ..opts(5.0) }).unwrap();
        let thin: Vec<&Leftover> = a.leftovers.iter().filter(|l| matches!(l.reason, Reason::TooThin { .. })).collect();
        assert_eq!(thin.len(), 2, "{a:?}");
        assert!(thin.iter().all(|l| l.reason == Reason::TooThin { gap: 2.0 }), "{a:?}");
        let c = completeness(&a, &[]);
        assert!(c.reported.iter().filter(|(_, why)| why.contains("nearer than the thinnest")).count() == 2, "{c:?}");
    }

    #[test]
    fn faces_within_the_limits_that_do_not_overlap_say_so() {
        // Two blocks side by side, 10 apart along x: one 2 tall, the other from z = 1 to 11. The
        // first's top (z = 2) and the second's bottom (z = 1) face away from each other 1 apart,
        // within 0.5 to 1.5, but nothing of one lies across from the other.
        let part = union(&cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 2.0]), &cuboid([20.0, 0.0, 1.0], [30.0, 10.0, 11.0]));
        assert_eq!(part.face_count(), 12);
        let a = analyze(&part, &Options { min_thickness: 0.5, ..opts(1.5) }).unwrap();
        assert!(a.pairs.is_empty(), "{:?}", a.pairs);
        let apart: Vec<&Leftover> = a.leftovers.iter().filter(|l| matches!(l.reason, Reason::NoOverlap { .. })).collect();
        assert_eq!(apart.len(), 2, "{a:?}");
        assert!(apart.iter().all(|l| l.reason == Reason::NoOverlap { gap: 1.0 }), "{a:?}");
    }

    #[test]
    fn a_rim_two_walls_share_is_covered_only_when_both_are_built() {
        // The L bracket built with its 40 long leg alone (38 x 20 before joining): that leg's tip
        // is its rim, but the two L-shaped ends run up the other leg too, which is not built.
        let bracket = l_bracket();
        let a = analyze(&bracket, &opts(5.0)).unwrap();
        let long = a.pairs.iter().find(|p| (p.overlap_area - 760.0).abs() < 1e-6).cloned().unwrap_or_else(|| panic!("{:?}", a.pairs));
        let c = completeness(&a, &[long]);
        // Paired: its 2 faces; rims: its tip; reported: the other leg's 2 faces, its tip and the
        // two L-shaped ends.
        assert_eq!((c.paired.len(), c.rims.len(), c.reported.len()), (2, 1, 5), "{c:?}");
        assert!(c.accounts_for_every_face());
        // Both built: all four are rims (an_l_brackets_rims_are_... above).
        assert_eq!(completeness(&a, &a.pairs).rims.len(), 4);
    }

    #[test]
    fn hostile_limits_are_refused() {
        let body = block(10.0, 10.0, 1.0);
        for o in [
            Options { min_thickness: f64::NAN, ..opts(5.0) },
            Options { max_thickness: f64::INFINITY, ..opts(5.0) },
            Options { min_thickness: 3.0, ..opts(2.0) },
            Options { tolerance: -1.0, ..opts(5.0) },
            Options { min_aspect: f64::NAN, ..opts(5.0) },
            Options { min_thickness: -1.0, ..opts(5.0) },
        ] {
            assert!(analyze(&body, &o).is_err(), "{o:?}");
        }
    }

    #[test]
    fn hostile_inputs_to_the_build_are_refused_not_panics() {
        let plate = block(40.0, 30.0, 2.0);
        let a = analyze(&plate, &opts(5.0)).unwrap();
        // No pairs: no sheets, and no body to make of them.
        assert!(sheets(&plate, &[]).unwrap().is_empty());
        assert!(surface_body(&[]).is_err());
        // Pairs of another body: the I-beam's faces 9 and 13 are past the plate's 6.
        let beam = i_beam();
        let other = analyze(&beam, &opts(5.0)).unwrap();
        assert!(sheets(&plate, &other.pairs).is_err());
        // A bad hole size.
        assert!(sheets_with(&plate, &a.pairs, &Build { carry_holes: true, suppress_holes_below: f64::NAN }).is_err());
        // A sheet with no corners.
        let mut s = sheets(&plate, &a.pairs).unwrap();
        s[0].corners.clear();
        assert!(surface_body(&s).is_err());
        // A sheet with NaN corners.
        let mut s = sheets(&plate, &a.pairs).unwrap();
        s[0].corners[0] = [f64::NAN, 0.0, 0.0];
        assert!(surface_body(&s).is_err());
        // A one-face surface body: nothing faces it, nothing pairs.
        let one = surface_body(&sheets(&plate, &a.pairs).unwrap()).unwrap();
        let alone = analyze(&one, &opts(5.0)).unwrap();
        assert!(alone.pairs.is_empty());
        assert_eq!(alone.leftovers.len(), 1, "{alone:?}");
        // A mesh body has no exact faces.
        let tri = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];
        let mesh = solidcraft_kernel::mesh_body(&tri, &[[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]).unwrap();
        assert!(analyze(&mesh, &opts(5.0)).is_err());
        assert!(sheets(&mesh, &[]).is_err());
        // Stored points on nothing.
        assert!(pairs_at(&plate, &a, &[[[f64::NAN, 0.0, 0.0], [0.0, 0.0, 0.0]]]).is_err());
    }

    /// A tube of radii `inner` and `outer` about the z axis, from `z0` to `z1`.
    fn tube(inner: f64, outer: f64, z0: f64, z1: f64) -> Body {
        let ring = Region2 { outer: Loop2::circle(Vec2::new(0.0, 0.0), outer), holes: vec![Loop2::circle(Vec2::new(0.0, 0.0), inner)] };
        extrude(&Plane::XY, &[ring], z0, z1).unwrap().pop().unwrap()
    }

    #[test]
    fn a_bore_stepped_between_two_counterbores_is_not_cut_out_at_the_counterbores_width() {
        // A plate 40 x 30 x 4 with counterbores of radius 5, 1 deep, at (20, 15) in its top and
        // its bottom, and a bore of radius 1 through between them. The faces' openings are both
        // radius 5, but at mid-thickness the wall is solid from radius 1 out: cutting the radius-5
        // mouth would take 24 pi of wall away. Not cut: the sheet is the whole 1200 mm², filled
        // over the 25 pi of mouth, and the pair needs a decision.
        let plate = block(40.0, 30.0, 4.0);
        let disc = |r: f64, z0: f64, z1: f64| {
            extrude(&Plane::XY, &[Region2 { outer: Loop2::circle(Vec2::new(20.0, 15.0), r), holes: vec![] }], z0, z1).unwrap().pop().unwrap()
        };
        let cut = |b: &Body, t: &Body| solidcraft_kernel::boolean(b, t, BoolOp::Cut).unwrap().unwrap();
        // The cutters run past the faces they open.
        let part = cut(&cut(&cut(&plate, &disc(5.0, 3.0, 5.0)), &disc(5.0, -1.0, 1.0)), &disc(1.0, -1.0, 5.0));
        let a = analyze(&part, &opts(5.0)).unwrap();
        let main = a.pairs.iter().find(|p| (p.thickness - 4.0).abs() < 1e-9).unwrap_or_else(|| panic!("{:?}", a.pairs));
        assert!((main.overlap_area - 1200.0).abs() < 1e-6, "{main:?}");
        assert!((main.filled_over - 25.0 * PI).abs() < 1e-3, "{main:?}");
        assert_eq!(main.confidence, Confidence::Review, "{main:?}");
        let s = sheets(&part, std::slice::from_ref(main)).unwrap();
        assert!(s.iter().all(|x| x.holes.is_empty()), "{s:?}");
        // Built on request, the reply says the sheet fills the stepped bore, and the pair, built,
        // is no longer one that needs a decision (by `report`, which knows no choice, too).
        let notes = report_chosen(&a, std::slice::from_ref(main), &s);
        assert!(notes.iter().any(|n| n.contains("built over a bore: stepped")), "{notes:?}");
        // The other pairs for review (none built) are still counted: all but `main`.
        let others = a.pairs.iter().filter(|p| p.confidence == Confidence::Review && p.faces != main.faces).count();
        let counted = |notes: &[String]| notes.iter().any(|n| n.contains("need a decision"));
        let expected = format!("{others} pairs need a decision");
        for notes in [notes.clone(), report(&a, &s)] {
            assert_eq!(counted(&notes), others > 0, "{notes:?}");
            assert!(others == 0 || notes.iter().any(|n| n.starts_with(&expected)), "{notes:?}");
        }
    }

    #[test]
    fn a_blind_pocket_in_a_tubes_outside_keeps_the_round_wall_from_being_built() {
        // The tube of radii 8 and 10, 50 long, with a pocket of radius 1 from outside at z = 25,
        // its floor 9.5 from the axis: the outer face has an opening the bore has not. Its chart
        // area changes by about pi in 2 pi x 500, so the extents and areas still agree; the
        // openings do not. The outer face's offset would carry a hole the wall has not.
        let pocket =
            extrude(&Plane::XZ, &[Region2 { outer: Loop2::circle(Vec2::new(0.0, -25.0), 1.0), holes: vec![] }], 9.5, 14.5).unwrap().pop().unwrap();
        let part = solidcraft_kernel::boolean(&tube(8.0, 10.0, 0.0, 50.0), &pocket, BoolOp::Cut).unwrap().unwrap();
        let a = analyze(&part, &opts(5.0)).unwrap();
        let round: Vec<&Pair> = a.pairs.iter().filter(|p| p.is_round()).collect();
        assert_eq!(round.len(), 1, "{:?}", a.pairs);
        assert_eq!(round[0].confidence, Confidence::Review, "{:?}", round[0]);
        assert!(round[0].notes.iter().any(|n| n.starts_with("opening")), "{:?}", round[0]);
        // Accepted anyway, it is not built.
        let s = sheets(&part, &a.pairs).unwrap();
        assert!(!s.iter().any(|x| x.faces == round[0].faces), "{s:?}");
    }

    #[test]
    fn a_cavity_closed_inside_a_tubes_wall_needs_a_decision() {
        // The tube of radii 8 and 10, 50 long, with a closed box-shaped void inside its wall:
        // x -0.4..0.4, y 8.6..9.4, z 20..30 (its corners lie 8.6 to 9.41 from the axis, all
        // inside). Neither face's outline or openings change, and its faces are flat, so no
        // round face lies between the two; its edges do: the pair needs a decision.
        let void = cuboid([-0.4, 8.6, 20.0], [0.4, 9.4, 30.0]);
        let part = solidcraft_kernel::boolean(&tube(8.0, 10.0, 0.0, 50.0), &void, BoolOp::Cut).unwrap().unwrap();
        let a = analyze(&part, &opts(5.0)).unwrap();
        let round: Vec<&Pair> = a.pairs.iter().filter(|p| p.is_round()).collect();
        assert_eq!(round.len(), 1, "{:?}", a.pairs);
        assert_eq!(round[0].confidence, Confidence::Review, "{:?}", round[0]);
        assert!(round[0].notes.iter().any(|n| n.starts_with("blocked")), "{:?}", round[0]);
        // The same tube without the void is one wall, built without a decision.
        let clear = analyze(&tube(8.0, 10.0, 0.0, 50.0), &opts(5.0)).unwrap();
        assert!(clear.pairs.iter().any(|p| p.is_round() && p.confidence == Confidence::High), "{:?}", clear.pairs);
    }

    #[test]
    fn a_short_small_ring_on_a_long_plate_is_still_paired() {
        // A plate 10,000 x 20 x 2 with a ring of radii 3 and 5, 2 tall (z = 2 to 4), standing on
        // it at the origin. A thousandth of the body is about 10: a probe that far from the
        // ring's faces misses a band 2 tall along the axis, and round a radius of 3 or 5 leaves
        // the cylinder by far more than half of it. The ring is still one round wall 2 thick.
        let part = union(&cuboid([-10.0, -10.0, 0.0], [9990.0, 10.0, 2.0]), &tube(3.0, 5.0, 2.0, 4.0));
        let loose = Options { min_thickness: 0.0, max_thickness: 2.5, min_aspect: 0.0, tolerance: 0.0 };
        let a = analyze(&part, &loose).unwrap();
        let round: Vec<&Pair> = a.pairs.iter().filter(|p| p.is_round()).collect();
        assert_eq!(round.len(), 1, "{:?}", a.pairs);
        assert!((round[0].thickness - 2.0).abs() < 1e-9, "{:?}", round[0]);
        // Its pick points lie on radii 5 and 3, on the band.
        for p in round[0].points {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            assert!(((r - 5.0).abs() < 1e-6 || (r - 3.0).abs() < 1e-6) && (2.0..=4.0).contains(&p[2]), "{p:?}");
        }
        // The plate's own wall is paired too.
        assert!(a.pairs.iter().any(|p| !p.is_round() && (p.thickness - 2.0).abs() < 1e-9), "{:?}", a.pairs);
    }

    #[test]
    fn two_tubes_one_inside_the_other_are_not_one_thick_wall() {
        // Tubes of radii 8 to 10 and 12 to 14, 50 long, joined by a ring of radii 8 to 14 from
        // z = 50 to 52. With limits up to 7 and no aspect limit, the bore (8) and the outside
        // (14) face each other 6 apart over the same length, but the faces at 10 and 12 lie
        // between them over it: air between two walls, not one wall 6 thick.
        let part = union(&union(&tube(8.0, 10.0, 0.0, 50.0), &tube(12.0, 14.0, 0.0, 50.0)), &tube(8.0, 14.0, 50.0, 52.0));
        let loose = Options { min_thickness: 0.0, max_thickness: 7.0, min_aspect: 0.0, tolerance: 0.0 };
        let a = analyze(&part, &loose).unwrap();
        let across: Vec<&Pair> = a.pairs.iter().filter(|p| p.is_round() && (p.thickness - 6.0).abs() < 1e-9).collect();
        assert!(!across.is_empty(), "{:?}", a.pairs);
        for p in across {
            assert_eq!(p.confidence, Confidence::Review, "{p:?}");
            assert!(p.notes.iter().any(|n| n.starts_with("blocked")), "{p:?}");
        }
        // One tube of radii 8 to 14: one wall 6 thick, its sheet at radius 11.
        let solid = tube(8.0, 14.0, 0.0, 50.0);
        let a = analyze(&solid, &loose).unwrap();
        let round: Vec<&Pair> = a.pairs.iter().filter(|p| p.is_round()).collect();
        assert!(round.len() == 1 && round[0].confidence == Confidence::High && (round[0].thickness - 6.0).abs() < 1e-9, "{:?}", a.pairs);
        let s = sheets(&solid, &confident(&a)).unwrap();
        let m = solidcraft_kernel::measure(&surface_body(&s).unwrap()).unwrap();
        // 2 pi 11 x 50 = 1100 pi.
        assert!((m.area - 1100.0 * PI).abs() < 1100.0 * PI * 1e-3, "{m:?}");
    }

    #[test]
    fn a_bore_and_an_outside_at_different_places_along_the_axis_are_no_pair() {
        // A tube of radii 8 to 20 from z = 0 to 10 on a solid round of radius 14 from z = 10 to
        // 30: the bore (8) and the round's outside (14) face each other 6 apart, within 0 to 7,
        // but over z 0..10 and z 10..30: they share no area, so they are no pair, and both say
        // that a face 6 away does not overlap them.
        let round =
            extrude(&Plane::XY, &[Region2 { outer: Loop2::circle(Vec2::new(0.0, 0.0), 14.0), holes: vec![] }], 10.0, 20.0).unwrap().pop().unwrap();
        let part = union(&tube(8.0, 20.0, 0.0, 10.0), &round);
        let loose = Options { min_thickness: 0.0, max_thickness: 7.0, min_aspect: 0.0, tolerance: 0.0 };
        let a = analyze(&part, &loose).unwrap();
        assert!(!a.pairs.iter().any(Pair::is_round), "{:?}", a.pairs);
        let apart = a.leftovers.iter().filter(|l| matches!(l.reason, Reason::NoOverlap { gap } if (gap - 6.0).abs() < 1e-9)).count();
        assert_eq!(apart, 2, "{:?}", a.leftovers);
    }

    #[test]
    fn a_flat_leg_meeting_a_bend_at_an_angle_is_reported_not_taken_as_tangent() {
        // A quarter bend of radii 8 and 10 about the z axis (0 to 90 degrees), 20 tall, with a
        // flat leg leaving its 0-degree end at 45 degrees down: its sides run from (8, 0) and
        // (10, 0) along (1, -1) / sqrt 2. The bend runs along y there, so leg and bend meet at
        // 45 degrees on both edges: no tangent line, and no joint is made. Said, not silent.
        let c = std::f64::consts::FRAC_1_SQRT_2;
        let p = |x: f64, y: f64| Vec2::new(x, y);
        let section = Loop2 {
            segs: vec![
                Seg2::Arc { center: p(0.0, 0.0), radius: 10.0, start: 0.0, sweep: FRAC_PI_2 },
                Seg2::Line { a: p(0.0, 10.0), b: p(0.0, 8.0) },
                Seg2::Arc { center: p(0.0, 0.0), radius: 8.0, start: FRAC_PI_2, sweep: -FRAC_PI_2 },
                Seg2::Line { a: p(8.0, 0.0), b: p(8.0 + 20.0 * c, -20.0 * c) },
                Seg2::Line { a: p(8.0 + 20.0 * c, -20.0 * c), b: p(10.0 + 20.0 * c, -20.0 * c) },
                Seg2::Line { a: p(10.0 + 20.0 * c, -20.0 * c), b: p(10.0, 0.0) },
            ],
        };
        let part = extrude(&Plane::XY, &[Region2 { outer: section, holes: vec![] }], 0.0, 20.0).unwrap().pop().unwrap();
        let a = analyze(&part, &opts(5.0)).unwrap();
        assert!(a.pairs.iter().any(|p| p.is_round()) && a.pairs.iter().any(|p| !p.is_round()), "{:?}", a.pairs);
        let s = sheets(&part, &a.pairs).unwrap();
        let notes = report(&a, &s);
        assert!(notes.iter().any(|n| n.contains("meet at an angle")), "{notes:?}");
    }
}
