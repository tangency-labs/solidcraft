//! Shaft Spline (#17693): the rebuild of a `shaft_spline` feature. Its numbers are worked out and handed to
//! [`crate::shaft_spline::plan`], and the kernel's tooth outlines become bodies or cuts:
//!
//! - Nothing picked: a new splined shaft and/or hub at the origin along +Z, from z = 0 to the length, each one
//!   extruded region (the shaft's outline; the hub a ring with the opening as its hole).
//! - A shaft picked: its cylinder is found from the stored pick, and the end nearest the pick is the free end. A
//!   spline whose major diameter is not the shaft's own makes the shaft follow it first: the whole round face is
//!   built up or turned down to the major diameter, so more teeth of one size make a larger shaft. Then the tooth
//!   spaces are cut from the free end for the length: with a cutter diameter each space is one tool that ends in
//!   the curve the cutter leaves (the space's own section turned a quarter turn about the cutter's axis), else
//!   one ring tool whose hole is the whole outline.
//! - A bore picked: one plug whose outline is the hub's opening is cut from the bore's end nearest the pick.
//! - The ends of a picked face: what the body has past each end is read off the exact body first ([`End`]). The
//!   cuts run out through a chamfer or a round the part already has there, as a cutter runs out of the work, at
//!   the far end too where the teeth run the whole face (no space then ends in the cutter's curve). A plain
//!   chamfer the part had keeps its size where the spline resizes the part. The spline's own chamfers (the end of
//!   a shaft's teeth, the entry of a bore) are cut on the round face before the spaces, so each is made again at
//!   every size and tooth count, on each open end the teeth run out of. A chamfer of the spline's own runs its
//!   size along the round face and, with nothing else given, as far across the end (45 degrees); a second
//!   distance or an angle sets how far across it runs instead.
//!
//! Angle 0 comes from the world axes: the world axis the cylinder's axis is closest to decides, and its reference
//! direction (+X for Z, +Y for X, +Z for Y) is projected onto the section. Decisions about the model use the
//! cylinder the kernel reads off the picked face and the exact bounds of the tools; the outlines are exact. Every
//! point, direction, length and angle of it (the seat on the round face, the outlines as profiles, the sections of
//! the tools) is the kernel's ([`crate::shaft_spline`]); this file sequences the steps and names the faces.

use std::collections::HashSet;
use std::f64::consts::TAU;
use std::sync::Arc;

use solidcore_record::round_face::End;
use solidcore_record::shaft_spline::{
    ChamferSection, ChamferSize, CutterSection, EndChamfer, LoopPiece, OutlinePiece as Piece, RoundFace, SeatFrame, SeatRefusal, SpaceRegion,
    SpaceSection, ToothOutline as Outline, ToothRole as Role,
};
use solidcore_record::{Loop2, Seg2};

use super::*;
use crate::Expr;
use crate::shaft_spline::{
    SplineMake, SplineMaster, SplineNumbers, SplinePlan, axis_gap, chamfer_plane, chamfer_section, copy_turn, cutter_radius, cutter_section,
    end_carried, end_run_out, end_run_past, far_end, lies_inward, lies_turned_back, moved_out, off_cylinder, points_apart, quarter_turn_in,
    same_size, seat_frame, shut_end, space_region, start_candidates, tool_margin, tooth_at, turn_tells_sense,
};

/// The most tooth spaces one Shaft Spline ends in the cutter's curve. Each is a tool of its own that the kernel's
/// boolean and its check meet, so the cap stays as low as a cell grid's.
pub const MAX_ROUNDED_SPACES: usize = 250;

/// The names of the new bodies.
const SHAFT_NAME: &str = "Spline Shaft";
const HUB_NAME: &str = "Spline Hub";

/// Evaluate a Shaft Spline feature.
pub(super) fn eval(vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState, warning: &mut Option<String>) -> Result<()> {
    let FeatureKind::ShaftSpline {
        shaft,
        shaft_start,
        bore,
        bore_start,
        form,
        make,
        teeth,
        length,
        module,
        pressure_angle,
        addendum,
        dedendum,
        root_fillet,
        profile_shift,
        thinning,
        hub_addendum,
        tip_clearance,
        form_clearance,
        major_diameter,
        minor_diameter,
        tooth_width,
        flank_angle,
        root_round,
        tip_corner,
        tip_size,
        major_clearance,
        minor_clearance,
        hub_tip_round,
        side_clearance,
        hub_root_round,
        hub_diameter,
        master,
        master_angle,
        cutter_diameter,
        end_chamfer,
        entry_chamfer,
        end_chamfer_distance2,
        end_chamfer_angle,
        entry_chamfer_distance2,
        entry_chamfer_angle,
    } = &f.kind
    else {
        return Err(DocError::Invalid("not a shaft spline".into()));
    };
    let opt = |e: &Option<Expr>, k: Kind| -> Result<Option<f64>> { e.as_deref().map(|e| val(vals, e, k)).transpose() };
    // Angles go to the kernel's plan in degrees.
    let degrees = |e: &Option<Expr>| -> Result<Option<f64>> { Ok(opt(e, Kind::Angle)?.map(f64::to_degrees)) };
    let z = val(vals, teeth, Kind::Unitless)?;
    if !(z.is_finite() && z.fract() == 0.0 && z >= 0.0 && z <= f64::from(u32::MAX)) {
        return Err(DocError::Invalid("`teeth` must be a whole number".into()));
    }
    let picked = shaft.is_some() || bore.is_some();
    // Both faces are found on the model as it was before the feature.
    let shaft_seat = shaft.as_ref().map(|p| seat_at(st, f.component, p.0, shaft_start.as_ref().map(|s| s.0), false)).transpose()?;
    let bore_seat = bore.as_ref().map(|p| seat_at(st, f.component, p.0, bore_start.as_ref().map(|s| s.0), true)).transpose()?;
    // A chamfer belongs to the part it is cut on: a shaft's teeth (a picked shaft's, a new shaft's), a hub's.
    let shaft_teeth = shaft.is_some() || (!picked && *make != SplineMake::Hub);
    let hub_teeth = bore.is_some() || (!picked && *make != SplineMake::Shaft);
    let mut n = SplineNumbers {
        form: *form,
        // With a face picked, what is cut follows the picks; no new part is made.
        make: if picked { SplineMake::Shaft } else { *make },
        teeth: z as u32, // whole and within u32, checked above
        length: val(vals, length, Kind::Length)?,
        module: opt(module, Kind::Length)?,
        pressure_angle: degrees(pressure_angle)?,
        addendum: opt(addendum, Kind::Unitless)?,
        dedendum: opt(dedendum, Kind::Unitless)?,
        root_fillet: opt(root_fillet, Kind::Unitless)?,
        profile_shift: opt(profile_shift, Kind::Unitless)?,
        thinning: opt(thinning, Kind::Length)?,
        hub_addendum: opt(hub_addendum, Kind::Unitless)?,
        tip_clearance: opt(tip_clearance, Kind::Length)?,
        form_clearance: opt(form_clearance, Kind::Length)?,
        major_diameter: opt(major_diameter, Kind::Length)?,
        minor_diameter: opt(minor_diameter, Kind::Length)?,
        tooth_width: opt(tooth_width, Kind::Length)?,
        flank_angle: degrees(flank_angle)?,
        root_round: opt(root_round, Kind::Length)?,
        tip_corner: *tip_corner,
        tip_size: opt(tip_size, Kind::Length)?,
        major_clearance: opt(major_clearance, Kind::Length)?,
        minor_clearance: opt(minor_clearance, Kind::Length)?,
        hub_tip_round: opt(hub_tip_round, Kind::Length)?,
        side_clearance: Some(val(vals, side_clearance, Kind::Length)?),
        hub_root_round: opt(hub_root_round, Kind::Length)?,
        // The outside of a new hub: a picked bore has its own.
        hub_diameter: if picked { None } else { opt(hub_diameter, Kind::Length)? },
        master: *master,
        master_angle: degrees(master_angle)?,
        // The cutter ends the tooth spaces of a picked shaft; nothing else is cut with it.
        cutter_diameter: if shaft.is_some() { opt(cutter_diameter, Kind::Length)? } else { None },
        end_chamfer: if shaft_teeth { opt(end_chamfer, Kind::Length)? } else { None },
        entry_chamfer: if hub_teeth { opt(entry_chamfer, Kind::Length)? } else { None },
        // How far across each chamfer runs: a second distance or an angle, with the chamfer it belongs to.
        end_chamfer_distance2: if shaft_teeth { opt(end_chamfer_distance2, Kind::Length)? } else { None },
        end_chamfer_angle: if shaft_teeth { degrees(end_chamfer_angle)? } else { None },
        entry_chamfer_distance2: if hub_teeth { opt(entry_chamfer_distance2, Kind::Length)? } else { None },
        entry_chamfer_angle: if hub_teeth { degrees(entry_chamfer_angle)? } else { None },
        with_hub: if picked { bore.is_some() } else { *make != SplineMake::Shaft },
        with_shaft: if picked { shaft.is_some() } else { *make != SplineMake::Hub },
    };
    // A size left out with a shaft picked is the shaft's own, so the shaft keeps its diameter; so is a size
    // given that is the shaft's own but for its last places. Any other size given wins, and the shaft follows it.
    if let Some(seat) = &shaft_seat {
        n = crate::shaft_spline::fit_to_shaft_within(&n, seat.radius);
    }
    let plan = crate::shaft_spline::plan(&n).map_err(|r| DocError::Invalid(r.words))?;
    if !picked {
        return new_parts(f, st, &n, &plan);
    }
    if let Some(seat) = &shaft_seat {
        cut_shaft(st, &n, &plan, seat, warning)?;
    }
    if let Some(seat) = &bore_seat {
        cut_bore(st, &n, &plan, seat, warning)?;
    }
    Ok(())
}

/// A round face the kernel read off a body, as the numbers the tooth geometry takes.
fn round_face(c: &kernel::CylinderFace) -> RoundFace {
    RoundFace { axis_point: c.axis_point, axis: *c.axis, radius: c.radius, start: c.start, end: c.end }
}

/// How far `p` is from the cylinder of the round face `c` (across the axis, not past its ends).
fn off_face(c: &kernel::CylinderFace, p: Vec3) -> f64 {
    off_cylinder(&round_face(c), p)
}

/// The start a Shaft Spline takes when none is given with the round face picked at `p` (a `bore`'s wall, else the
/// outside of a shaft): a point in the plane of the end of the round face nearest the pick, on the side of the
/// rim the part's end face lies (inside the rim of a shaft, outside the rim of a bore) and well clear of the rim,
/// where the body has a face. On a plain end that is a point of the end face, so stored as the start it follows
/// that face when the part changes. Where the body has no face in that plane (a shoulder stands there, a thin
/// wall), the point on the axis: the end's place as it is now. None where no round face of the kind asked for
/// lies.
pub fn spline_start_at(st: &ModelState, p: Vec3, bore: bool) -> Option<Vec3> {
    let (body, cyl) = st
        .bodies
        .iter()
        .filter_map(|b| kernel::cylinder_face_at(&b.body, p).map(|c| (b, c)))
        .filter(|(_, c)| c.internal == bore)
        .min_by(|a, b| off_face(&a.1, p).total_cmp(&off_face(&b.1, p)))?;
    // Half, a quarter and a tenth of the radius off the rim, then the end's place on the axis.
    let [half, quarter, tenth, end] = start_candidates(&round_face(&cyl), p, bore)?;
    // The first of the three the body has a face at.
    let on_a_face = |at: &Vec3| body.body.nearest_face(*at).is_some_and(|(far, _)| far <= 1e-9 * (1.0 + cyl.radius));
    let at = [half, quarter, tenth].into_iter().find(on_a_face).unwrap_or(end);
    (at.x.is_finite() && at.y.is_finite() && at.z.is_finite()).then_some(at)
}

/// The radius of the outside of the round shaft at `p` on the model `st`: what a Shaft Spline with its size left
/// out takes as the spline's major radius. None where no shaft's round face lies.
pub fn shaft_radius_at(st: &ModelState, p: Vec3) -> Option<f64> {
    st.bodies
        .iter()
        .filter_map(|b| kernel::cylinder_face_at(&b.body, p))
        .filter(|c| !c.internal)
        .min_by(|a, b| off_face(a, p).total_cmp(&off_face(b, p)))
        .map(|c| c.radius)
        .filter(|r| r.is_finite() && *r > 0.0)
}

// ---------------------------------------------------------------------------------------------------------------
// Outlines as profiles
// ---------------------------------------------------------------------------------------------------------------

/// The points where two segments of one curve of a profile meet, by their exact bits: a wall swept from a run of
/// such segments is one face.
#[derive(Default)]
struct Joints(HashSet<(u64, u64)>);

impl Joints {
    fn add(&mut self, p: Vec2) {
        self.0.insert((p.x.to_bits(), p.y.to_bits()));
    }

    /// Whether `t` carries on the curve `s` is a piece of: two free-form segments meeting at one of the points.
    fn joins(&self, s: &Seg2, t: &Seg2) -> bool {
        match (seg_end(s), seg_start(t)) {
            (Some(a), Some(b)) => a == b && self.0.contains(&(b.x.to_bits(), b.y.to_bits())),
            _ => false,
        }
    }
}

/// The first point of a free-form segment.
fn seg_start(s: &Seg2) -> Option<Vec2> {
    match *s {
        Seg2::Cubic { p0, .. } => Some(p0),
        Seg2::Conic { a, .. } => Some(a),
        Seg2::Bezier { p, .. } => p.first().copied(),
        Seg2::Line { .. } | Seg2::Arc { .. } => None,
    }
}

/// The last point of a free-form segment.
fn seg_end(s: &Seg2) -> Option<Vec2> {
    match *s {
        Seg2::Cubic { p3, .. } => Some(p3),
        Seg2::Conic { b, .. } => Some(b),
        Seg2::Bezier { n, p } => p.get(usize::from(n)).copied(),
        Seg2::Line { .. } | Seg2::Arc { .. } => None,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------------------------------------------
//
// Every face this feature makes is named by what it is, not by a running number, so that what is built on a
// spline afterwards (a chamfer on a flank, a sketch on a root) finds its face again when the spline is edited:
// `space<k>:flank:0`, `space<k>:root`, `space<k>:end:flank:0` (the rounded end), `tip<k>`, `side` (the shaft's
// surface where the spline made it). `k` is the tooth's number counted round from angle 0: the space after tooth
// `k` is space `k`. The names are read off the kernel's own record of which profile segment each face was swept
// from; nothing is found by place.

/// What a piece of an outline is called in the name of the face swept from it.
fn role_word(role: Role) -> &'static str {
    match role {
        Role::Flank => "flank",
        Role::Fillet => "fillet",
        Role::Tip => "tip",
        Role::Root => "root",
    }
}

/// The names of the pieces of the floor of a tooth space, in the floor's order (from the corner of the tooth after
/// the space down its side, along the bottom and up the side of the tooth before): what each is on the part, and,
/// where there are several of a kind, which of them on the way. So `flank:0` is the side of the tooth after the
/// space, `flank:1` that of the tooth before, and a space's one bottom is `root`.
fn floor_names(floor: &[Role]) -> Vec<String> {
    let several = |word: &str| floor.iter().filter(|r| role_word(**r) == word).count() > 1;
    let mut met: Vec<(&'static str, usize)> = Vec::new();
    floor
        .iter()
        .map(|role| {
            let word = role_word(*role);
            let nth = match met.iter_mut().find(|(w, _)| *w == word) {
                Some((_, n)) => {
                    *n += 1;
                    *n - 1
                }
                None => {
                    met.push((word, 1));
                    0
                }
            };
            if several(word) { format!("{word}:{nth}") } else { word.to_string() }
        })
        .collect()
}

/// The name of span `span` of a piece called `name`: the piece's own for its first span, `name.<span>` for the
/// others. A piece swept as one face answers to all of them and is called by the first; a piece the kernel sweeps
/// span by span (an arc in a cutter's section) has a name for each of its faces.
fn span_name(name: &str, span: usize) -> String {
    if span == 0 { name.to_string() } else { format!("{name}.{span}") }
}

/// A tool this feature built from a profile, its faces named as its building records them: a face swept from
/// profile segments has the names `segment` gives them (a wall along a whole curve is swept from several spans, and
/// is called by the first of their names), a cap the name `caps` gives its end (the start, then the end). A face
/// with an origin that has no name is left without one: never a partial guess.
fn built(body: Body, segment: &dyn Fn(usize) -> Option<String>, caps: [&str; 2]) -> Traced {
    let roles = body.history().filter(|h| h.operands == 0).map(|h| {
        h.faces
            .iter()
            .map(|face| {
                let mut roles: Vec<String> = Vec::new();
                for origin in &face.origins {
                    let role = match *origin {
                        kernel::history::FaceOrigin::Profile { segment: k } => segment(k),
                        kernel::history::FaceOrigin::Cap { end } => caps.get(usize::from(end)).map(|cap| (*cap).to_string()),
                        _ => None,
                    };
                    match role {
                        Some(role) if !roles.contains(&role) => roles.push(role),
                        Some(_) => {}
                        None => return Vec::new(),
                    }
                }
                roles.sort();
                roles
            })
            .collect()
    });
    Traced::made(body, roles)
}

/// A plain round tool (a sleeve, a ring) swept from `region`, whose outer loop is one circle and whose hole, if
/// any, another: the wall swept from the outer circle is called `outer`, that from the hole `inner`, the caps
/// `start` and `end`.
fn built_round(body: Body, region: &Region2, outer: &str, inner: &str) -> Traced {
    let first = region.outer.segs.len();
    built(body, &|s| Some(if s < first { outer } else { inner }.to_string()), ["start", "end"])
}

/// `tool` (a tooth space's tool as first made, or a copy moved from it) as the tool of tooth space `k`: the names
/// of the faces this feature built take the space's number after their first word, so `space:flank:0` is
/// `space<k>:flank:0`. The tool first made stands for every equal space, so its own names carry no number.
fn for_space(tool: &Traced, k: u32) -> Traced {
    let numbered = |role: &String| match role.split_once(':') {
        Some((part, rest)) => format!("{part}{k}:{rest}"),
        None => format!("{role}{k}"),
    };
    let lineage = tool.lineage.as_ref().map(|l| crate::lineage::Lineage {
        history: l.history.clone(),
        operands: l
            .operands
            .iter()
            .map(|op| match op.as_ref() {
                crate::lineage::Operand::Made(roles) => {
                    Arc::new(crate::lineage::Operand::Made(roles.iter().map(|names| names.iter().map(numbered).collect()).collect()))
                }
                _ => op.clone(),
            })
            .collect(),
    });
    Traced { body: tool.body.clone(), lineage, recipe: None }
}

/// Where the teeth of an outline are counted from, for the names of its faces: `teeth` teeth as the spline's
/// numbers give them (a master spline's outline has one fewer top or space), tooth 0's top at angle `zero`.
#[derive(Clone, Copy)]
struct Count {
    teeth: u32,
    zero: f64,
}

impl Count {
    /// The number of the tooth whose top is nearest the angle `at`: the kernel's answer.
    fn tooth_at(&self, at: f64) -> u32 {
        tooth_at(self.teeth, self.zero, at)
    }
}

/// The name of each segment of a whole outline's loop, whose pieces in order are `pieces`: a top is
/// `<what it is><k>` (`tip3` on a shaft, where the tops are the tips of the teeth); the pieces between the top of
/// tooth `k` and the next top are `<between><k>:<name>` with the names [`floor_names`] gives them in a tooth
/// space's own order (`space3:flank:1`, `space3:root`, `space3:flank:0`), so that a face has one name whether its
/// space was cut as a whole outline or by a tool of its own. An outline with no tops (teeth that come to a point)
/// numbers the pieces between one point of the largest circle and the next. A name met again is told apart by a
/// number after it.
fn outline_names(pieces: &[LoopPiece], segments: usize, between: &str, count: Count) -> Vec<String> {
    let mut piece_names: Vec<String> = vec![String::new(); pieces.len()];
    let tops: Vec<usize> = pieces.iter().enumerate().filter(|(_, p)| p.top).map(|(i, _)| i).collect();
    for &i in &tops {
        if let (Some(p), Some(name)) = (pieces.get(i), piece_names.get_mut(i)) {
            *name = format!("{}{}", role_word(p.role), count.tooth_at(p.middle));
        }
    }
    // Where each run starts and which tooth it follows: after a top, or, on an outline with no tops, at the piece
    // that starts on a point of the largest circle.
    let starts: Vec<(usize, u32, bool)> = if tops.is_empty() {
        pieces.iter().enumerate().filter_map(|(i, p)| p.corner.map(|at| (i, count.tooth_at(at), false))).collect()
    } else {
        tops.iter().filter_map(|&i| pieces.get(i).map(|p| (i, count.tooth_at(p.middle), true))).collect()
    };
    // Each run of pieces from one start round to the next, in the loop's order; the last run goes round the end.
    let runs: Vec<(u32, Vec<usize>)> = if starts.is_empty() {
        vec![(0, (0..pieces.len()).collect())]
    } else {
        starts
            .iter()
            .enumerate()
            .map(|(t, &(from, k, after_a_top))| {
                let to = starts.get(t + 1).map_or_else(|| starts.first().map_or(0, |s| s.0) + pieces.len(), |s| s.0);
                let first = if after_a_top { from + 1 } else { from };
                (k, (first..to).map(|i| i % pieces.len().max(1)).collect())
            })
            .collect()
    };
    for (k, run) in runs {
        // A tooth space's own order runs the other way round the axis.
        let roles: Vec<Role> = run.iter().rev().filter_map(|&i| pieces.get(i).map(|p| p.role)).collect();
        for (&i, name) in run.iter().rev().zip(floor_names(&roles)) {
            if let Some(slot) = piece_names.get_mut(i) {
                *slot = format!("{between}{k}:{name}");
            }
        }
    }
    // A name met again (two tops within one tooth's pitch, as on a master spline's wide tooth).
    let mut met: Vec<(String, usize)> = Vec::new();
    for name in &mut piece_names {
        match met.iter_mut().find(|(n, _)| n == name) {
            Some((_, n)) => {
                *name = format!("{name}:{n}");
                *n += 1;
            }
            None => met.push((name.clone(), 1)),
        }
    }
    let mut names = vec![String::new(); segments];
    for (p, name) in pieces.iter().zip(&piece_names) {
        for span in 0..p.segs {
            let Some(slot) = names.get_mut(p.first + span) else { continue };
            *slot = span_name(name, span);
        }
    }
    names
}

/// The whole outline as one closed loop, every tooth turned to its place and the whole turned by `turn`, with the
/// joints of its curves added to `joints`, and the name of each of its segments ([`outline_names`]; the pieces
/// between two tops are called `between`, numbered as `count` counts the teeth once the loop is turned).
fn outline_loop(outline: &Outline, turn: f64, joints: &mut Joints, between: &str, count: Count) -> Result<(Loop2, Vec<String>)> {
    let made = crate::shaft_spline::outline_loop(outline, turn).map_err(|r| DocError::Invalid(r.words))?;
    for q in &made.joints {
        joints.add(*q);
    }
    let names = outline_names(&made.pieces, made.segs.len(), between, count);
    Ok((Loop2 { segs: made.segs }, names))
}

/// One body: `region` on `plane` extruded from `from` to `to` along its normal, each run of segments of one curve
/// swept into one wall.
fn extruded(plane: &Plane, region: &Region2, from: f64, to: f64, joints: &Joints, what: &str) -> Result<Body> {
    let one_curve = |s: &Seg2, t: &Seg2| joints.joins(s, t);
    let mut bodies = kernel::extrude_whole_sides(plane, std::slice::from_ref(region), from, to, &one_curve)?;
    match (bodies.pop(), bodies.is_empty()) {
        (Some(body), true) => Ok(body),
        _ => Err(DocError::Invalid(format!("the {what} is not one solid"))),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// New parts
// ---------------------------------------------------------------------------------------------------------------

/// The new shaft and/or hub at the origin along +Z, from z = 0 to the length: the bodies the command made as a
/// sketch and an extrude.
fn new_parts(f: &Feature, st: &mut ModelState, n: &SplineNumbers, plan: &SplinePlan) -> Result<()> {
    let mut made = 0usize;
    let mut add = |st: &mut ModelState, body: Traced, base: &str| {
        let base = f.body_names.get(made).map(String::as_str).filter(|name| !name.trim().is_empty()).unwrap_or(base);
        let name = unique_body_name(st, base);
        st.bodies.push(ModelBody::traced_new(name, body, f.id));
        made += 1;
    };
    let count = count_of(n, plan);
    if n.make != SplineMake::Hub {
        let mut joints = Joints::default();
        let (outer, names) = outline_loop(&plan.shaft, 0.0, &mut joints, "space", count)?;
        let region = Region2 { outer, holes: Vec::new() };
        let body = extruded(&Plane::XY, &region, 0.0, n.length, &joints, "splined shaft")?;
        let mut part = built(body, &|s| names.get(s).cloned(), ["bottom", "top"]);
        if let Some(size) = plan.end_chamfer {
            let (root, top) = (plan.shaft.least_radius, plan.shaft.most_radius);
            let margin = tool_margin(top - root);
            part = chamfered_ends(part, top, size, margin, n.length, seam_angle(plan, false), true, OwnChamfer::End.keys(n))?;
        }
        add(st, part, SHAFT_NAME);
    }
    if n.make != SplineMake::Shaft {
        let od = n.hub_diameter.ok_or_else(|| DocError::Invalid("`hub_diameter` is required to make the hub (mm, its outside diameter)".into()))?;
        let mut joints = Joints::default();
        let (opening, names) = outline_loop(&plan.hub, plan.hub_start, &mut joints, "tooth", count)?;
        let outer = Loop2::circle(Vec2::ZERO, od / 2.0);
        let first = outer.segs.len();
        let region = Region2 { outer, holes: vec![opening] };
        let body = extruded(&Plane::XY, &region, 0.0, n.length, &joints, "hub")?;
        let mut part = built(body, &|s| if s < first { Some("side".to_string()) } else { names.get(s - first).cloned() }, ["bottom", "top"]);
        if let Some(size) = plan.entry_chamfer {
            let (minor, major) = (plan.hub.least_radius, plan.hub.most_radius);
            let margin = tool_margin(major - minor);
            part = chamfered_ends(part, minor, size, margin, n.length, seam_angle(plan, true), false, OwnChamfer::Entry.keys(n))?;
        }
        add(st, part, HUB_NAME);
    }
    Ok(())
}

/// A new part with the spline's own chamfer on both its ends (the teeth run out of both): on the ends of a
/// shaft's teeth (`outside`), at the two entries of a hub's bore. The part stands on the XY plane and runs up for
/// `length`; `radius` is that of the tops of its teeth.
#[allow(clippy::too_many_arguments)]
fn chamfered_ends(
    part: Traced,
    radius: f64,
    size: ChamferSize,
    margin: f64,
    length: f64,
    seam: f64,
    outside: bool,
    keys: ChamferKeys,
) -> Result<Traced> {
    let seat = Seat { body: String::new(), plane: Plane::XY, inward: 1.0, radius, room: length, face: None, at: Vec3::new(radius, 0.0, 0.0) };
    let chamfer = EndChamfer { radius, radial: size.radial, axial: size.axial, past: 0.0, flush: None };
    let section = chamfer_section(&chamfer, margin, outside)
        .map_err(|_| DocError::Invalid(if outside { keys.reaches_the_axis(size, radius - margin) } else { keys.not_in_bore(radius) }))?;
    let key = keys.size;
    let mut part = part;
    for (at, name) in [(seat.clone(), "chamfer"), (other_end(&seat), "chamfer:far")] {
        let tool = chamfer_tool(&at, &section, seam, name)?;
        let body = kernel::boolean(&part.body, &tool.body, BoolOp::Cut)?
            .ok_or_else(|| DocError::Invalid(format!("`{key}`: the chamfer removed everything")))?;
        part = Traced::step(body, &[&part, &tool]);
    }
    Ok(part)
}

/// How the teeth are counted for the names of the faces: the spline's own number of teeth, tooth 0's top at angle
/// 0, or where a master spline puts it.
fn count_of(n: &SplineNumbers, plan: &SplinePlan) -> Count {
    let zero = match (&plan.master, n.master) {
        (Some(m), SplineMaster::ToothOut | SplineMaster::WideTooth) => m.turn,
        _ => 0.0,
    };
    Count { teeth: plan.made.teeth, zero }
}

// ---------------------------------------------------------------------------------------------------------------
// Picked faces
// ---------------------------------------------------------------------------------------------------------------

/// Where a spline is cut: the body, and the round face's free end.
#[derive(Clone)]
struct Seat {
    body: String,
    /// The plane square to the axis at the free end: its x is angle 0, its normal the axis as the world axes
    /// orient it, so that angles turn counter-clockwise seen from the positive end of that world axis.
    plane: Plane,
    /// 1 when the face runs on from the free end along the plane's normal, -1 when against it.
    inward: f64,
    /// The radius of the face.
    radius: f64,
    /// The length of the face along its axis.
    room: f64,
    /// Which face of the body, as it was before the feature, the round face is (None where that is not known
    /// exactly): the face a surface the spline rebuilds over it stands for.
    face: Option<usize>,
    /// A point of the round face: where it was picked, brought onto the face's cylinder.
    at: Vec3,
}

impl Seat {
    /// The distances along the plane's normal of the stretch from `from` to `to` measured into the face from its
    /// free end.
    fn span(&self, from: f64, to: f64) -> (f64, f64) {
        if self.inward > 0.0 { (from, to) } else { (-to, -from) }
    }

    /// The seat as the kernel takes it: the plane at the free end, the way into the face, its length and a point
    /// of it.
    fn frame(&self) -> SeatFrame {
        SeatFrame { plane: self.plane, inward: self.inward, room: self.room, at: self.at }
    }
}

/// The round face the stored pick `p` means and the body it is on (its place in `st.bodies`): a bore (`bore`) or
/// the outside of a shaft, on the body nearest the pick, the feature's own component's bodies first, as a thread
/// finds its face.
///
/// A hub that sits on its shaft with no play has its bore on the shaft's own surface, so the pick lies on both
/// faces: the one of the kind asked for is the one meant. A face of the other kind is given (for [`seat_at`] to
/// refuse in words) only where no face of the kind asked for lies.
fn round_face_at(st: &ModelState, component: u64, p: Vec3, bore: bool) -> Option<(usize, kernel::CylinderFace)> {
    let (asked, other): (Vec<(usize, kernel::CylinderFace)>, Vec<(usize, kernel::CylinderFace)>) = st
        .bodies
        .iter()
        .enumerate()
        .filter(|(_, b)| !super::more::beyond_cylinder_reach(b, p))
        .filter_map(|(i, b)| kernel::cylinder_face_at(&b.body, p).map(|c| (i, c)))
        .partition(|(_, c)| c.internal == bore);
    let found = if asked.is_empty() { other } else { asked };
    let own = |i: &usize| st.bodies.get(*i).is_some_and(|b| b.component == component);
    let any_own = found.iter().any(|(i, _)| own(i));
    found.into_iter().filter(|(i, _)| !any_own || own(i)).min_by(|a, b| off_face(&a.1, p).total_cmp(&off_face(&b.1, p)))
}

/// The name of the body a Shaft Spline's pick `p` means ([`round_face_at`]): the body the pick's lasting face
/// name is taken on, so the name is that of the face the rebuild cuts into. None where no round face of the kind
/// asked for lies.
pub(crate) fn seat_body(st: &ModelState, component: u64, p: Vec3, bore: bool) -> Option<String> {
    let (i, cyl) = round_face_at(st, component, p, bore)?;
    if cyl.internal != bore {
        return None;
    }
    st.bodies.get(i).map(|b| b.name.clone())
}

/// The round face at the stored pick `p` ([`round_face_at`]), with the end the teeth start from as the free end:
/// the end nearest along the axis to `start`, a point on the face they start from, or with none stored the end
/// nearest the pick.
fn seat_at(st: &ModelState, component: u64, p: Vec3, start: Option<Vec3>, bore: bool) -> Result<Seat> {
    let what = if bore { "bore" } else { "shaft" };
    let (i, cyl) = round_face_at(st, component, p, bore).ok_or_else(|| {
        DocError::Invalid(format!(
            "no round face where the {what} was picked: pick the {}",
            if bore { "inside of a round bore" } else { "outside of a round shaft" }
        ))
    })?;
    if cyl.internal != bore {
        return Err(DocError::Invalid(if bore {
            "the face picked as the bore is the outside of a shaft: pick the inside of a bore".into()
        } else {
            "the face picked as the shaft is the inside of a bore: pick the outside of a shaft".into()
        }));
    }
    let body = st.bodies.get(i).map(|b| b.name.clone()).ok_or_else(|| DocError::Invalid(format!("the {what}'s body")))?;
    // The teeth start at the end the start face lies at; the round face runs on from it into the part.
    let frame = seat_frame(&round_face(&cyl), p, start).map_err(|why| {
        DocError::Invalid(match why {
            SeatRefusal::NoDirection => format!("the {what}'s axis has no direction"),
            SeatRefusal::NoPlane => format!("no plane square to the {what}'s axis"),
            SeatRefusal::NoSize => format!("the {what}'s round face has no size"),
        })
    })?;
    // The face itself, by the exact distance of the pick from each face: the nearest, when it lies on this cylinder
    // (the same radius about the same axis, not another round face of that size beside it).
    let face = st.bodies.get(i).and_then(|b| {
        let (_, face) = b.body.nearest_face(p)?;
        let on_it = match kernel::face_surface(&b.body, face) {
            Some(kernel::FaceSurface::Cylinder { origin, axis, radius }) => {
                let gap = axis_gap(cyl.axis_point, *cyl.axis, origin, *axis);
                (radius - cyl.radius).abs() <= 1e-9 * (1.0 + cyl.radius) && gap.lean <= 1e-9 && gap.off <= 1e-6 * (1.0 + cyl.radius)
            }
            _ => false,
        };
        on_it.then_some(face)
    });
    Ok(Seat { body, plane: frame.plane, inward: frame.inward, radius: cyl.radius, room: frame.room, face, at: frame.at })
}

/// The seat after its round face has moved `by` out from the axis (into a bore: a negative `by`): the face has the
/// new radius, and the point on it has moved with it.
fn moved_seat(seat: &Seat, by: f64) -> Seat {
    Seat { radius: seat.radius + by, at: moved_out(&seat.plane, seat.at, by), ..seat.clone() }
}

/// Moves the seat's round face along its outward normal by `by` with the kernel's face move (what Offset Face
/// does): the face goes onto the cylinder of the new radius, and every face next to it is carried on, or cut back,
/// on its own surface. What opens in the round face (a hole across a shaft, a keyway, a flat, a set screw's hole in
/// a hub) therefore stays open through the new surface, with its own walls, and a round on an end follows at its
/// own size. The face keeps its name. Kept from the last rebuilds as the cuts are ([`KeptChange`]).
fn move_round_face(st: &mut ModelState, seat: &Seat, by: f64, key: &str) -> Result<()> {
    let body = &seat.body;
    let i = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Invalid(format!("the body `{body}` is gone")))?;
    let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid(format!("the body `{body}` is gone"))) };
    let input = mb.traced();
    let kept = {
        let all = KEPT_CHANGES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        all.iter().find(|k| k.key == key && k.input.same(&input.body)).map(|k| k.output.clone())
    };
    let nb = match kept {
        Some(found) => found,
        None => {
            let nb = kernel::offset_faces(&input.body, &[seat.at], by)?;
            let mut all = KEPT_CHANGES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if all.first().is_some_and(|k| spline_of(&k.key) != spline_of(key)) {
                all.clear();
            }
            if all.len() >= MOST_KEPT {
                drop(all.remove(0));
            }
            all.push(KeptChange { input: input.body.clone(), key: key.to_string(), output: nb.clone(), tool: None });
            nb
        }
    };
    if let Some(slot) = st.bodies.get_mut(i) {
        *slot = mb.remade(Traced::step(nb, &[&input]));
    }
    Ok(())
}

/// Says that the faces the tools' face called `role` left on the body called `body` stand for the body's own
/// face `face` too (its face before this feature): a shaft's surface built up or turned down is still the shaft's
/// surface, a bore filled in is still the bore. They are then called by that face's own name, and the tool's name
/// is another name for them, so what a later feature picked on the surface is found again whether or not the
/// spline resized the part this time.
fn also_from(st: &mut ModelState, body: &str, role: &str, face: Option<usize>) {
    let Some(face) = face else { return };
    let Some(i) = st.bodies.iter().position(|b| b.name == body) else { return };
    let Some(mb) = st.bodies.get(i).cloned() else { return };
    let traced = mb.traced();
    let Some(mut lineage) = traced.lineage else { return };
    // The body as it was before the feature, among what the body now comes from.
    let Some(own) = lineage.operands.iter().position(|op| matches!(op.as_ref(), crate::lineage::Operand::Body(name) if name == body)) else {
        return;
    };
    let origin = kernel::history::FaceOrigin::Face { operand: own, face };
    let crate::lineage::Lineage { history, operands } = &mut lineage;
    let of_role = |operand: usize, tool_face: usize| {
        operands.get(operand).is_some_and(|op| {
            matches!(op.as_ref(), crate::lineage::Operand::Made(roles) if roles.get(tool_face).and_then(|names| names.first()).is_some_and(|name| name == role))
        })
    };
    let mut changed = false;
    for prov in history.faces.iter_mut() {
        let from_tool =
            prov.origins.iter().any(|o| matches!(*o, kernel::history::FaceOrigin::Face { operand, face: tool_face } if of_role(operand, tool_face)));
        if from_tool && !prov.origins.contains(&origin) {
            prov.origins.push(origin);
            changed = true;
        }
    }
    if changed && let Some(slot) = st.bodies.get_mut(i) {
        *slot = mb.remade(Traced { body: traced.body, lineage: Some(lineage), recipe: traced.recipe });
    }
}

/// Joins or cuts `tool` into the body called `body`, which keeps its name. The body's own faces keep their names,
/// and the faces the tool leaves are called by the tool's.
fn change(st: &mut ModelState, body: &str, tool: &Traced, op: BoolOp, what: &str) -> Result<()> {
    let i = st.bodies.iter().position(|b| b.name == body).ok_or_else(|| DocError::Invalid(format!("the body `{body}` is gone")))?;
    let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid(format!("the body `{body}` is gone"))) };
    let input = mb.traced();
    let nb = kernel::boolean(&input.body, &tool.body, op)?.ok_or_else(|| DocError::Invalid(format!("{what} removed everything")))?;
    if let Some(slot) = st.bodies.get_mut(i) {
        *slot = mb.remade(Traced::step(nb, &[&input, tool]));
    }
    Ok(())
}

/// A change kept from the last rebuilds: the body it was made on, what the tool was made from, the body it gave
/// and what the tool's faces came from (their names). An edit through the History rebuilds a feature twice with
/// the same numbers (once to try them, once for real): the second time the very same body meets the very same
/// tool, and the cut is not made again.
struct KeptChange {
    input: Body,
    key: String,
    output: Body,
    tool: Option<crate::lineage::Lineage>,
}

/// The changes of the spline rebuilt last ([`KeptChange`]), oldest first. Another spline (other numbers, another
/// face) starts the list again, so it holds one feature's steps and no more than [`MOST_KEPT`].
static KEPT_CHANGES: std::sync::Mutex<Vec<KeptChange>> = std::sync::Mutex::new(Vec::new());

/// The most changes kept: more than the steps of the largest spline (its rounded ends are cut [`END_GROUP`] at a
/// time, [`MAX_ROUNDED_SPACES`] at most, after a few steps on the round shaft).
const MOST_KEPT: usize = 64;

/// What ends the part of a step's key that says which spline it belongs to ([`tool_key`]).
const KEY_END: &str = "|#";

/// The part of a step's key that says which spline it belongs to.
fn spline_of(key: &str) -> &str {
    key.split_once(KEY_END).map_or(key, |(spline, _)| spline)
}

/// [`change`] with a tool that is made only when this change is not one of the last few kept. `key` says all the
/// tool is made from; `tool` gives the tool.
fn change_kept(st: &mut ModelState, body: &str, key: &str, op: BoolOp, what: &str, tool: impl FnOnce() -> Result<Traced>) -> Result<()> {
    let i = st.bodies.iter().position(|b| b.name == body).ok_or_else(|| DocError::Invalid(format!("the body `{body}` is gone")))?;
    let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid(format!("the body `{body}` is gone"))) };
    let input = mb.traced();
    let kept = {
        let all = KEPT_CHANGES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        all.iter().find(|k| k.key == key && k.input.same(&input.body)).map(|k| (k.output.clone(), k.tool.clone()))
    };
    let (nb, tool) = match kept {
        Some(found) => found,
        None => {
            let tool = tool()?;
            let nb = kernel::boolean(&input.body, &tool.body, op)?.ok_or_else(|| DocError::Invalid(format!("{what} removed everything")))?;
            let mut all = KEPT_CHANGES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if all.first().is_some_and(|k| spline_of(&k.key) != spline_of(key)) {
                all.clear();
            }
            if all.len() >= MOST_KEPT {
                drop(all.remove(0));
            }
            all.push(KeptChange { input: input.body.clone(), key: key.to_string(), output: nb.clone(), tool: tool.lineage.clone() });
            (nb, tool.lineage)
        }
    };
    // The body's own faces keep their names, and the faces the tool leaves are called by the tool's: the kept
    // output is the very body this tool gave on this input, so the same record of where its faces came from holds.
    let lineage = match (input.lineage.as_ref(), tool.as_ref()) {
        (Some(body), Some(tool)) => crate::lineage::Lineage::step(&nb, &[body, tool]),
        _ => None,
    };
    if let Some(slot) = st.bodies.get_mut(i) {
        *slot = mb.remade(Traced { body: nb, lineage, recipe: None });
    }
    Ok(())
}

/// All a tool of this feature is made from, but for what each tool adds: the numbers and where the face is.
fn tool_key(n: &SplineNumbers, seat: &Seat) -> String {
    format!("{n:?}|{:?}|{}|{}{KEY_END}", seat.plane, seat.inward, seat.radius)
}

/// Refused when the splined length is longer than the picked face.
fn check_room(seat: &Seat, length: f64, what: &str) -> Result<()> {
    if length > seat.room + same_size(seat.radius) {
        return Err(DocError::Invalid(format!("`length` is longer than the {what}'s round face ({} mm): make it at most that", show_mm(seat.room))));
    }
    Ok(())
}

/// The seat seen from the other end of its round face: the far end as the free end, with angle 0 where it was.
fn other_end(seat: &Seat) -> Seat {
    let far = far_end(&seat.frame());
    Seat { plane: far.plane, inward: far.inward, ..seat.clone() }
}

/// What the body has past the seat's free end, as the kernel reads it off the exact body before the spline
/// ([`End`]; for the far end, the seat seen from there: [`other_end`]). A shaft's end may carry a chamfer or a
/// round, or step down to a neck; a bore's entry may carry a chamfer, or open into a counterbore. `reach` is the
/// radius the cuts reach (the bottoms of a shaft's tooth spaces, of a hub's in a `bore`). An end of a body that is
/// gone, or that cannot be read, is shut: the tools stop at it.
fn past_end(st: &ModelState, seat: &Seat, bore: bool, reach: f64) -> End {
    match st.bodies.iter().find(|b| b.name == seat.body) {
        Some(mb) => kernel::round_face_past_end(&mb.body, &seat.frame(), seat.radius, bore, reach),
        None => shut_end(seat.radius),
    }
}

/// The largest radius the seat's body reaches in the stretch `from`..`to` (measured from the free end into the
/// face) outside the radius `above`; `above` itself where it has nothing there. The kernel's reading of the exact
/// body. None where it cannot be told.
fn stands_over(st: &ModelState, seat: &Seat, above: f64, from: f64, to: f64) -> Option<f64> {
    let mb = st.bodies.iter().find(|b| b.name == seat.body)?;
    kernel::round_face_stands_over(&mb.body, &seat.frame(), seat.radius, above, from, to).ok()
}

/// The radius the rounded ends of the tooth spaces must reach out past, from [`stands_over`]'s reading: a reading
/// that could not be made is refused, not taken as nothing standing there. A shoulder taller than the teeth would
/// otherwise stand over tools that stop short of it, and the spaces would end shut beneath it.
fn cutter_way_out(over: Option<f64>) -> Result<f64> {
    over.ok_or_else(|| {
        DocError::Invalid(
            "`cutter_diameter`: what stands past the end of the teeth, where the cutter runs out, could not be read from the shaft: end the teeth further from the step, or leave `cutter_diameter` out (the spaces then end square)"
                .into(),
        )
    })
}

/// Whether the seat's round face is a whole band, as the kernel reads it off the exact body: every face of the
/// body on the seat's cylinder is bounded only by circles across the axis on that cylinder (its ends) and by
/// straight lines along the axis (seams). A hole across a shaft, a flat, a keyway or a set screw's hole in a hub
/// that opens in the face leaves other edges in it. None where it cannot be read: the body is not found, or no
/// face of it lies on the cylinder.
fn whole_band(st: &ModelState, seat: &Seat) -> Option<bool> {
    let body = &st.bodies.iter().find(|b| b.name == seat.body)?.body;
    kernel::round_face_whole_band(body, &seat.frame(), seat.radius).ok()
}

/// Builds a shaft's round face up by `by` where the grown face must pass something larger that stands next to
/// one of its ends (a collar, a larger step), which a face moved on the whole body cannot. The stretch of the body
/// the face runs along is taken out of the body: cut off at the plane of an end where the body goes on past the
/// face, and reaching `before` past the near end and `after` past the far end where the shaft has its own end
/// there (with a chamfer or a round on it). In the stretch nothing stands in the face's way: the face is moved as
/// on a shaft of its own (the kernel call of [`move_round_face`]), what opens in it carried on and a round on its
/// end following; then the stretch is joined on again.
fn grow_stretch(st: &mut ModelState, seat: &Seat, by: f64, before: f64, after: f64) -> Result<()> {
    let body = &seat.body;
    let i = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Invalid(format!("the body `{body}` is gone")))?;
    let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid(format!("the body `{body}` is gone"))) };
    let input = mb.traced();
    // A round block over the stretch, wider than the body and than the grown face: the kernel's numbers.
    let block = kernel::round_face_grow_block(&input.body, &seat.frame(), seat.radius, by, before, after)
        .map_err(|_| DocError::Invalid("the shaft's size could not be read".into()))?;
    let slab = Region2 { outer: Loop2::circle(Vec2::ZERO, block.radius), holes: Vec::new() };
    let made = extruded(&block.plane, &slab, block.from, block.to, &Joints::default(), "block over the shaft's round face")?;
    let tool = built_round(made, &slab, "outside", "inner");
    let part = kernel::boolean(&input.body, &tool.body, BoolOp::Intersect)?
        .ok_or_else(|| DocError::Invalid("nothing of the shaft lies along its round face".into()))?;
    let part = Traced::step(part, &[&input, &tool]);
    let moved = Traced::step(kernel::offset_faces(&part.body, &[seat.at], by)?, &[&part]);
    // What lies past the stretch; nothing where the stretch is the whole body.
    let out = match kernel::boolean(&input.body, &tool.body, BoolOp::Cut)? {
        Some(rest) => {
            let rest = Traced::step(rest, &[&input, &tool]);
            let joined = kernel::boolean(&rest.body, &moved.body, BoolOp::Union)?
                .ok_or_else(|| DocError::Invalid("joining the shaft's round stretch on again left nothing".into()))?;
            Traced::step(joined, &[&rest, &moved])
        }
        None => moved,
    };
    if let Some(slot) = st.bodies.get_mut(i) {
        *slot = mb.remade(out);
    }
    Ok(())
}

/// Whether the body leaves the stretch just past the seat's end empty within `radius` of the axis: a chamfer
/// that wide at a bore's step cuts into no wall there.
fn beside_clear(st: &ModelState, seat: &Seat, radius: f64) -> bool {
    st.bodies
        .iter()
        .find(|b| b.name == seat.body)
        .is_some_and(|mb| kernel::round_face_beside_clear(&mb.body, &seat.frame(), seat.radius, radius).unwrap_or(false))
}

/// An angle about the axis that lies in a tooth space of the shaft (`hub`: of the hub), for the seam of a tool
/// turned once about the axis: a seam there is cut away with the space, so no tooth's chamfer is in two faces. The
/// plan's own.
fn seam_angle(plan: &SplinePlan, hub: bool) -> f64 {
    if hub { plan.hub_seam } else { plan.shaft_seam }
}

/// Which of the spline's own chamfers: on the ends of a shaft's teeth, or at the entries of a hub's bore.
#[derive(Clone, Copy)]
enum OwnChamfer {
    End,
    Entry,
}

impl OwnChamfer {
    /// The parameters a refusal about this chamfer names, as the numbers `n` give it.
    fn keys(self, n: &SplineNumbers) -> ChamferKeys {
        let (size, distance2, angle, given) = match self {
            OwnChamfer::End => ("end_chamfer", "end_chamfer_distance2", "end_chamfer_angle", (n.end_chamfer_distance2, n.end_chamfer_angle)),
            OwnChamfer::Entry => {
                ("entry_chamfer", "entry_chamfer_distance2", "entry_chamfer_angle", (n.entry_chamfer_distance2, n.entry_chamfer_angle))
            }
        };
        let (across, by_angle) = match given {
            (Some(_), _) => (distance2, false),
            (None, Some(_)) => (angle, true),
            (None, None) => (size, false),
        };
        ChamferKeys { size, across, by_angle }
    }
}

/// The parameters a refusal about one of the spline's own chamfers names.
#[derive(Clone, Copy)]
struct ChamferKeys {
    /// The chamfer's own: its size along the round face.
    size: &'static str,
    /// The one that sets how far the chamfer runs across the end: its second distance or its angle where one is
    /// given, else its size.
    across: &'static str,
    /// `across` is the chamfer's angle.
    by_angle: bool,
}

impl ChamferKeys {
    /// A shaft's chamfer that would reach the axis: it must run less than `most` across the end.
    fn reaches_the_axis(&self, size: ChamferSize, most: f64) -> String {
        let key = self.across;
        if self.by_angle {
            format!(
                "`{key}` takes the chamfer {} mm in from the major diameter, and it must be less than {} mm: the chamfer would reach the axis; make the angle smaller",
                show_mm(size.radial),
                show_mm(most)
            )
        } else {
            format!("`{key}` must be less than {} mm: the chamfer would reach the axis", show_mm(most))
        }
    }

    /// A bore's chamfer whose tool has no section clear of the axis in a bore of `radius`.
    fn not_in_bore(&self, radius: f64) -> String {
        format!("`{}` cannot be cut in a bore of {} mm", self.across, show_mm(2.0 * radius))
    }
}

/// The spline's own chamfer of `size` on the end `e` of a round face that is now `radius`, where one can be cut:
/// at an end the body ends at, or at a step. None at a shoulder, and at an end that runs on with no corner.
fn own_chamfer(e: &End, radius: f64, size: ChamferSize, outside: bool) -> Option<EndChamfer> {
    if !e.open {
        return None;
    }
    let (radial, axial) = (size.radial, size.axial);
    if e.free || e.end > 0.0 {
        return Some(EndChamfer { radius, radial, axial, past: e.end, flush: None });
    }
    if !e.step {
        return None;
    }
    if outside {
        // A neck as large as the shaft leaves no corner.
        (e.neck < radius - 1e-6 * (1.0 + radius)).then_some(EndChamfer { radius, radial, axial, past: 0.0, flush: Some(e.neck) })
    } else {
        Some(EndChamfer { radius, radial, axial, past: 0.0, flush: Some(f64::INFINITY) })
    }
}

/// The tool of a chamfer at the seat's end: its `section` ([`chamfer_section`]) turned once about the face's axis,
/// from the angle `seam`. Its sloping face is called `name`.
fn chamfer_tool(seat: &Seat, section: &ChamferSection, seam: f64, name: &str) -> Result<Traced> {
    let ChamferSection { points, slope } = section;
    // A plane through the axis: across the shaft at the seam's angle, and along it into the face.
    let through = chamfer_plane(&seat.plane, seat.inward, seam);
    let segs = points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| Seg2::Line { a: *a, b: *b }).collect();
    let region = Region2 { outer: Loop2 { segs }, holes: Vec::new() };
    let mut bodies = kernel::revolve(&through, std::slice::from_ref(&region), Vec2::ZERO, Vec2::new(0.0, 1.0), TAU)?;
    let body = match (bodies.pop(), bodies.is_empty()) {
        (Some(body), true) => body,
        _ => return Err(DocError::Invalid("the chamfer's tool is not one solid".into())),
    };
    Ok(built(body, &|k| Some(if k == *slope { name.to_string() } else { format!("{name}:tool:{k}") }), ["chamfer:tool:start", "chamfer:tool:end"]))
}

/// The spline cut into a picked shaft. Where the spline's major diameter is not the shaft's own, the shaft follows
/// the spline first: the whole of its round face is built up or turned down to the major diameter, so the tops of
/// the teeth are the shaft's surface whatever the numbers (more teeth of one size make a larger shaft). Then the
/// tooth spaces are cut: each as one tool with its rounded end when a cutter diameter is given, else one ring tool
/// for the straight part.
fn cut_shaft(st: &mut ModelState, n: &SplineNumbers, plan: &SplinePlan, seat: &Seat, warning: &mut Option<String>) -> Result<()> {
    let outline = &plan.shaft;
    let (root, top) = (outline.least_radius, outline.most_radius);
    let length = n.length;
    check_room(seat, length, "shaft")?;
    let tol = same_size(seat.radius);
    let depth = (top - root).max(0.0);
    // How far the tools reach past the faces they would otherwise only touch.
    let margin = tool_margin(depth);
    let key = tool_key(n, seat);
    let resized = (top - seat.radius).abs() > tol;
    // The two ends of the round face, as the shaft has them before this feature: a chamfer or a round the shaft
    // already has on an end is cut through, as a cutter runs out of the work, and follows the shaft to its new size.
    // The far end matters where the shaft is resized (the whole face is) and where the teeth run to it.
    let old_radius = seat.radius;
    let teeth_end = length >= seat.room - tol;
    let beyond = other_end(seat);
    let near = past_end(st, seat, false, root);
    let far = if resized || teeth_end { past_end(st, &beyond, false, root) } else { shut_end(old_radius) };
    let seam = seam_angle(plan, false);
    // How far a tool that takes the whole round face runs on past one of its ends: past an end the body ends at,
    // and through what the shaft has on its end (a chamfer, a round). At a shoulder and at a neck it stops with
    // the face.
    let past = |e: &End| end_run_past(e, margin);
    if resized && top > seat.radius {
        // The round face itself moves out to the major diameter, and every face next to it is carried on on its
        // own surface: a hole across the shaft, a keyway or a flat stays open through the new surface, with its own
        // walls, as if the shaft had been that large when they were cut, and a round on an end follows at its own
        // size. (A sleeve joined over the face would close what opens in it.)
        //
        // A face moved on the whole body cannot pass something larger that stands next to one of its ends (a
        // collar, a larger step). Whether it would is read off the exact body, within the growth's own length past
        // each end that is not open; such a face is moved on its own stretch of the body.
        let reach = top - old_radius + margin;
        let passes = [(&near, seat), (&far, &beyond)]
            .into_iter()
            .any(|(e, at)| !e.open && stands_over(st, at, old_radius, -reach, 0.0).is_some_and(|r| r > old_radius + tol && r < top + tol));
        let grown = if passes {
            grow_stretch(st, seat, top - seat.radius, past(&near), past(&far))
        } else {
            move_round_face(st, seat, top - seat.radius, &format!("{key}|moved"))
        };
        grown.map_err(|e| {
            let size = if n.form == crate::shaft_spline::SplineForm::Involute { "module" } else { "major_diameter" };
            DocError::Invalid(format!(
                "`{size}` makes the spline {} mm across, larger than the shaft's {} mm, and the shaft's round face could not be moved out to it ({e}): give the shaft that diameter where it is made, or tick Fixed diameter (leave `{size}` out, so the spline takes the shaft's own diameter)",
                show_mm(2.0 * top),
                show_mm(2.0 * seat.radius)
            ))
        })?;
        for (e, at, which) in [(&near, seat, "near"), (&far, &beyond, "far")] {
            // A plain chamfer on an end went with the moved face along its own cone, which meets the new surface
            // further into the face: the chamfer is larger now. The end is built up square again, to the end of
            // the body: a ring from just inside the rim of the shaft's end face, reaching as far into the face as
            // the cone does. The chamfer is cut again below, at its own size.
            if !(e.end > 0.0 && e.cone && old_radius > e.rim) {
                continue;
            }
            let carried = end_carried(e, old_radius, top);
            let (from, to) = at.span(-e.end, carried);
            let rim = e.rim;
            change_kept(
                st,
                &seat.body,
                &format!("{key}|built {which} end|{}|{rim}|{carried}", e.end),
                BoolOp::Union,
                "building the shaft's end up",
                || {
                    let inner = rim - margin;
                    let holes = if inner > 0.0 { vec![Loop2::circle(Vec2::ZERO, inner)] } else { Vec::new() };
                    let ring = Region2 { outer: Loop2::circle(Vec2::ZERO, top), holes };
                    let body = extruded(&at.plane, &ring, from, to, &Joints::default(), "ring that builds the shaft's end up")?;
                    Ok(built_round(body, &ring, "side", "inner"))
                },
            )?;
        }
    } else if resized {
        // A plain ring over the whole face, running on past an end the body ends at, and through what the shaft
        // has on its end. At a shoulder and at a neck it stops with the face.
        let (from, to) = seat.span(-past(&near), seat.room + past(&far));
        change_kept(st, &seat.body, &format!("{key}|turned|{from}|{to}"), BoolOp::Cut, "turning the shaft down", || {
            let ring = Region2 { outer: Loop2::circle(Vec2::ZERO, seat.radius + depth + margin), holes: vec![Loop2::circle(Vec2::ZERO, top)] };
            let body = extruded(&seat.plane, &ring, from, to, &Joints::default(), "ring that turns the shaft down")?;
            // Its hole leaves the shaft's surface: `side`, as a cylinder's wall is called.
            Ok(built_round(body, &ring, "outside", "side"))
        })?;
    }
    if resized {
        // The new surface is still the shaft's surface: it keeps the name the shaft's own face had.
        also_from(st, &seat.body, "side", seat.face);
    }
    // From here the shaft's round face is the spline's major diameter, and the spaces are cut as into a shaft that
    // had it from the start.
    let made;
    let seat = if resized {
        made = moved_seat(seat, top - seat.radius);
        &made
    } else {
        seat
    };
    let beyond = other_end(seat);
    // A plain chamfer the shaft had on an end keeps its size on the resized shaft: cut again at the new diameter.
    if resized {
        for (e, at, name) in [(&near, seat, "blank:chamfer"), (&far, &beyond, "blank:chamfer:far")] {
            if !(e.end > 0.0 && e.cone) {
                continue;
            }
            let chamfer = EndChamfer { radius: top, radial: old_radius - e.rim, axial: e.end, past: e.end, flush: None };
            let Ok(section) = chamfer_section(&chamfer, margin, true) else { continue };
            change_kept(st, &seat.body, &format!("{key}|{name}|{chamfer:?}"), BoolOp::Cut, "the chamfer the shaft had on its end", || {
                chamfer_tool(at, &section, seam, name)
            })?;
        }
    }
    // The spline's own chamfer on the ends of the teeth, cut on the round shaft before the spaces, as a blank is
    // turned before its spline is cut: made again at whatever size the shaft now has. It goes on each end the
    // teeth run out of: the free end, and the far end where the teeth run the whole face.
    if let Some(size) = plan.end_chamfer {
        let keys = OwnChamfer::End.keys(n);
        let mut cut = false;
        for (e, at, name, at_far) in [(&near, seat, "chamfer", false), (&far, &beyond, "chamfer:far", true)] {
            if at_far && !teeth_end {
                continue;
            }
            let Some(chamfer) = own_chamfer(e, top, size, true) else { continue };
            let section = chamfer_section(&chamfer, margin, true).map_err(|_| DocError::Invalid(keys.reaches_the_axis(size, top - margin)))?;
            change_kept(st, &seat.body, &format!("{key}|{name}|{chamfer:?}"), BoolOp::Cut, "the chamfer on the end of the teeth", || {
                chamfer_tool(at, &section, seam, name)
            })?;
            cut = true;
        }
        if !cut {
            append_warning(
                warning,
                "the end chamfer was not cut: the teeth run out at no open end of the shaft (a shoulder stands at the end they start from)".into(),
            );
        }
    }
    // The cuts start past the face's free end, and run out through what the shaft has on its end. Where the teeth
    // run the whole face and out of an open far end, the cutter runs right through the shaft: the cuts run out
    // there too, and no space ends in the cutter's curve.
    // At a shoulder the cuts start with the face: nothing past it is cut into.
    let before = if near.open { -(end_run_out(&near) + margin) } else { 0.0 };
    let through_far = teeth_end && far.out;
    let stop = if through_far { seat.room + end_run_out(&far) + margin } else { length };
    // A cutter's diameter is checked whether or not this rebuild rounds an end with it, so the same number is
    // taken or refused whatever the length.
    if let Some(diameter) = n.cutter_diameter {
        cutter_radius(diameter, root, top.max(seat.radius) + margin, margin).map_err(|r| DocError::Invalid(r.words))?;
    }
    let cutter = n.cutter_diameter.filter(|_| !through_far);
    if let Some(diameter) = cutter {
        // The cutter's curve runs on past the length, as far as the cutter's radius, through whatever stands above
        // the teeth there: a shoulder, a larger step. The tools reach out past it as the cutter does, so the space
        // comes out as an open slot; a tool that reached only past the teeth would bore under the step and leave a
        // ring of it over the spaces.
        let above = top.max(seat.radius);
        let over = cutter_way_out(stands_over(st, seat, above, length, length + diameter / 2.0))?;
        let out_to = over + margin;
        if over > above {
            // The cutter's section must lie on one side of its axis: a step higher than the cutter's centre is
            // more than this cutter can run out through.
            let least = out_to - root + margin;
            if !(diameter / 2.0 > least) {
                return Err(DocError::Invalid(format!(
                    "`cutter_diameter` must be more than {} mm to run out through the step past the end of the teeth, which stands {} mm above them: use a larger cutter, end the teeth further from the step, or leave `cutter_diameter` out (the spaces then end square)",
                    show_mm(2.0 * least),
                    show_mm(over - top)
                )));
            }
        }
        // Each tooth space is one tool: its straight part and its rounded end together.
        let straight = seat.span(before, length);
        return rounded_ends(st, n, plan, seat, diameter, margin, out_to, straight, &format!("{key}|spaces|{diameter}|{before}|{out_to}"), warning);
    }
    // The straight part: everything outside the outline, from just past the free end for the length.
    let (from, to) = seat.span(before, stop);
    change_kept(st, &seat.body, &format!("{key}|ring|{from}|{to}"), BoolOp::Cut, "the spline", || {
        let mut joints = Joints::default();
        let (hole, names) = outline_loop(outline, 0.0, &mut joints, "space", count_of(n, plan))?;
        let outer = Loop2::circle(Vec2::ZERO, top.max(seat.radius) + depth + margin);
        let first = outer.segs.len();
        let ring = Region2 { outer, holes: vec![hole] };
        let body = extruded(&seat.plane, &ring, from, to, &joints, "ring that cuts the spline")?;
        Ok(built(body, &|s| if s < first { Some("outside".to_string()) } else { names.get(s - first).cloned() }, ["start", "end"]))
    })
}

/// Each tooth space of a picked shaft ends in the cutter's curve: the cutter's section in the space (see
/// [`cutter_section`]) turned a quarter turn about the cutter's axis. That axis lies in the plane where the full
/// depth ends, square to the space's middle line, at the root radius plus the cutter's radius from the shaft's
/// axis. The quarter turn goes into the shaft, away from its free end, so the cut rises to the shaft's surface,
/// and through a shoulder where the shaft was turned down. Equal spaces are made once and copied round the axis.
/// A space wider than one cutter's section reaches round ends square instead, with a warning.
#[allow(clippy::too_many_arguments)]
fn rounded_ends(
    st: &mut ModelState,
    n: &SplineNumbers,
    plan: &SplinePlan,
    seat: &Seat,
    diameter: f64,
    margin: f64,
    out_to: f64,
    straight: (f64, f64),
    key: &str,
    warning: &mut Option<String>,
) -> Result<()> {
    // The tools lie apart (each within its own space), so several can be one body without any intersection work.
    // How many tools one cut takes: the kernel's work on a tool of many pieces that lie apart grows far faster
    // than their count, so the spaces of a spline of many teeth are cut some at a time.
    let spaces = plan.spaced_teeth.len();
    let cuts = spaces.div_ceil(END_GROUP).max(1);
    // The spaces that end square are told on every rebuild, whether or not their cut is kept from the last one.
    let square = square_spaces(plan, diameter, margin, out_to)?;
    if square > 0 {
        append_warning(
            warning,
            format!(
                "{square} of the {spaces} tooth spaces end{} square, not in the cutter's curve: a space this wide is more than one cutter's curve reaches round",
                if square == 1 { "s" } else { "" }
            ),
        );
    }
    // The tools are made when the first cut that is not one of the kept ones needs them. Tools that touch
    // (teeth that come to a point) cannot be one body: they are cut one by one instead.
    let mut made: Option<EndTools> = None;
    let mut loose = false;
    for cut in 0..cuts {
        let done = change_kept(st, &seat.body, &format!("{key}|{cut}/{cuts}"), BoolOp::Cut, "the rounded ends of the tooth spaces", || {
            if made.is_none() {
                made = Some(end_tools(n, plan, seat, diameter, margin, out_to, straight)?);
            }
            let all = made.as_ref().ok_or_else(|| DocError::Invalid("the rounded ends were not made".into()))?;
            let group = all.tools.chunks(END_GROUP).nth(cut).unwrap_or(&[]);
            let joined = match group {
                // One tool left over for the last cut is its own body.
                [one] => Some(one.clone()),
                _ if all.apart && !group.is_empty() => {
                    let bodies: Vec<Body> = group.iter().map(|t| t.body.clone()).collect();
                    kernel::join_apart(&bodies)?.map(|one| Traced::step(one, &group.iter().collect::<Vec<&Traced>>()))
                }
                _ => None,
            };
            match joined {
                Some(one) => Ok(one),
                None => {
                    loose = true;
                    Err(DocError::Invalid("the tooth spaces touch".into()))
                }
            }
        });
        if let Err(e) = done {
            return match made.take() {
                Some(made) if loose && cut == 0 => cut_each(st, seat, made),
                _ => Err(e),
            };
        }
    }
    Ok(())
}

/// How many rounded ends one cut takes.
const END_GROUP: usize = 16;

/// The tools of the rounded ends, and whether they lie apart.
struct EndTools {
    tools: Vec<Traced>,
    apart: bool,
}

/// The rounded ends cut one tool at a time.
fn cut_each(st: &mut ModelState, seat: &Seat, made: EndTools) -> Result<()> {
    for tool in &made.tools {
        change(st, &seat.body, tool, BoolOp::Cut, "the rounded end of a tooth space")?;
    }
    Ok(())
}

/// The sections of the shaft's tooth spaces a cutter of `diameter` ends: for each tooth with a space of its own
/// after it (the plan's), the tooth's number, the space and the cutter's section in it; no section for a space
/// wider than one cutter's section reaches round.
#[allow(clippy::type_complexity)]
fn space_sections(plan: &SplinePlan, diameter: f64, margin: f64, out_to: f64) -> Result<Vec<(u32, SpaceSection, Option<CutterSection>)>> {
    let outline = &plan.shaft;
    let root = outline.least_radius;
    // The section must lie on one side of the cutter's axis, with room to spare.
    let rc = cutter_radius(diameter, root, out_to, margin).map_err(|r| DocError::Invalid(r.words))?;
    let mut all = Vec::new();
    for spaced in &plan.spaced_teeth {
        let space = crate::shaft_spline::space_section(outline, spaced.angle, out_to)
            .map_err(|r| DocError::Invalid(format!("the rounded end of a tooth space: {}", r.words)))?;
        let section = cutter_section(&space, root, rc, out_to).map_err(|r| DocError::Invalid(r.words))?;
        all.push((spaced.tooth, space, section));
    }
    Ok(all)
}

/// How many of the shaft's tooth spaces are too wide to end in the curve of a cutter of `diameter`
/// ([`space_sections`]).
fn square_spaces(plan: &SplinePlan, diameter: f64, margin: f64, out_to: f64) -> Result<usize> {
    Ok(space_sections(plan, diameter, margin, out_to)?.iter().filter(|(_, _, section)| section.is_none()).count())
}

/// The tools of [`rounded_ends`]: one for each tooth space, each the whole space: the cutter's section swept along
/// the straight part (`straight`, its span along the normal of the shaft's end plane) and then turned out of the
/// shaft, as one body. The section reaches from the bottom of the space out to `out_to`: past whatever stands
/// above the teeth where the cutter runs out (the shaft's own surface where it was turned down to the teeth, a
/// larger step past the end of the teeth). A space too wide for a cutter's section is its whole cross-section
/// swept along the straight part only: it ends square.
fn end_tools(n: &SplineNumbers, plan: &SplinePlan, seat: &Seat, diameter: f64, margin: f64, out_to: f64, straight: (f64, f64)) -> Result<EndTools> {
    let outline = &plan.shaft;
    let top = outline.most_radius;
    let spaces = space_sections(plan, diameter, margin, out_to)?;
    if spaces.is_empty() {
        return Err(DocError::Invalid("`teeth` must be more than 0".into()));
    }
    let count = spaces.len();
    if count > MAX_ROUNDED_SPACES {
        return Err(DocError::Invalid(format!(
            "the spline has {count} tooth spaces to end in the cutter's curve, more than the {MAX_ROUNDED_SPACES} a Shaft Spline rounds: leave out `cutter_diameter` (the spaces then end square) or use fewer teeth"
        )));
    }
    let at_end = seat.plane.offset(seat.inward * n.length);
    // The sense of the quarter turn: the kernel turns a section towards its plane's normal; into the shaft is
    // against it when the shaft runs on against the normal. Checked on the first tool by its exact bounds.
    let mut quarter_back = false;
    let mut checked = false;
    let mut tools = Vec::new();
    let mut apart = true;
    let same = 1e-9 * (1.0 + top);
    let near = |a: [f64; 2], b: [f64; 2]| points_apart(a, b) <= same;
    let mut made: Vec<MadeSpace> = Vec::new();
    // Which way the kernel turns a copy about the shaft's axis: settled on the first copy by where it lies.
    let mut copy_back = false;
    // The shaft's axis as the section's plane has it (its normal).
    let about = seat.plane.normal();
    let mut sense_checked = false;
    let (low, high) = straight;
    for (k, space, section) in spaces {
        apart &= space.apart;
        let Some(CutterSection { segs, spans, middle, from, to, joints: points, axis_at, axis_dir }) = section else {
            // Too wide for one cutter's section: the space's whole cross-section, swept along the straight part.
            let SpaceRegion { segs, spans, joints: points } = space_region(&space, out_to).map_err(|r| DocError::Invalid(r.words))?;
            let names = cutter_names(&space.floor, &spans);
            let mut joints = Joints::default();
            for q in &points {
                joints.add(*q);
            }
            let region = Region2 { outer: Loop2 { segs }, holes: Vec::new() };
            let prism = extruded(&seat.plane, &region, low, high, &joints, "tooth space that ends square")?;
            let prism = built(prism, &|s| names.get(s).map(|name| format!("space:{name}")), ["space:start", "space:stop"]);
            tools.push(for_space(&prism, k));
            continue;
        };
        let names = cutter_names(&space.floor, &spans);
        // Where two spans of one curve meet: the turned tool has one face for the whole curve.
        let mut joints = Joints::default();
        for q in &points {
            joints.add(*q);
        }
        let pieces = segs.len();
        let like = made.iter().find(|m| m.pieces == pieces && near(m.from, from) && near(m.to, to));
        let copied = match like {
            Some(m) => {
                let mut body = kernel::transform(&m.tool.body, Vec3::ZERO, at_end.origin, about, copy_turn(m.middle, middle, copy_back))?;
                if !sense_checked && turn_tells_sense(m.middle, middle) {
                    // A copy must lie about the space's own middle line, not about that line's mirror image.
                    let bb = body.exact_bounds()?;
                    if lies_turned_back(&at_end, bb.min, bb.max, m.middle, middle) {
                        copy_back = true;
                        body = kernel::transform(&m.tool.body, Vec3::ZERO, at_end.origin, about, copy_turn(m.middle, middle, copy_back))?;
                    }
                    sense_checked = true;
                }
                // The copy's faces are the first tool's, each with this space's number in its name.
                Some(Traced::step(body, &[&for_space(&m.tool, k)]))
            }
            None => None,
        };
        let tool = match copied {
            Some(tool) => tool,
            None => {
                let region = Region2 { outer: Loop2 { segs }, holes: Vec::new() };
                let mut body = revolved(&at_end, &region, axis_at, axis_dir, quarter_turn_in(seat.inward, quarter_back), &joints)?;
                if !checked {
                    // The tool must lie past the plane where the full depth ends, into the shaft.
                    let bb = body.exact_bounds()?;
                    if !lies_inward(&at_end, seat.inward, bb.min, bb.max) {
                        quarter_back = true;
                        body = revolved(&at_end, &region, axis_at, axis_dir, quarter_turn_in(seat.inward, quarter_back), &joints)?;
                    }
                    checked = true;
                }
                // Its faces by what they are, with no number yet: the first tool stands for every equal space.
                let end = built(body, &|s| names.get(s).map(|name| format!("space:end:{name}")), ["space:end:start", "space:end:stop"]);
                // The same section along the straight part: its end is the rounded end's start, face on face.
                let prism = extruded(&seat.plane, &region, low, high, &joints, "straight part of a tooth space")?;
                let prism = built(prism, &|s| names.get(s).map(|name| format!("space:{name}")), ["space:start", "space:stop"]);
                let whole = kernel::boolean(&prism.body, &end.body, BoolOp::Union)?
                    .ok_or_else(|| DocError::Invalid("the tool of a tooth space came to nothing".into()))?;
                let tool = Traced::step(whole, &[&prism, &end]);
                made.push(MadeSpace { pieces, from, to, middle, tool: tool.clone() });
                for_space(&tool, k)
            }
        };
        tools.push(tool);
    }
    Ok(EndTools { tools, apart })
}

/// A tooth space's tool as first made, kept so that equal spaces are copies of it: how many pieces its section has,
/// its two corners as they lie with its middle line on angle 0, the angle of that line, and the tool.
struct MadeSpace {
    pieces: usize,
    from: [f64; 2],
    to: [f64; 2],
    middle: f64,
    tool: Traced,
}

/// The name of each segment of a cutter's section in a tooth space whose floor is `floor`
/// ([`crate::shaft_spline::cutter_section`], which made `spans[i]` segments of the floor's piece `i`): each span of a
/// piece by the piece's name ([`floor_names`], [`span_name`]), then the wall above the tooth before the space, the
/// top, and the wall above the tooth after it.
fn cutter_names(floor: &[Piece], spans: &[usize]) -> Vec<String> {
    let roles: Vec<Role> = floor.iter().map(Piece::role).collect();
    let mut names: Vec<String> = Vec::new();
    for (name, count) in floor_names(&roles).into_iter().zip(spans) {
        names.extend((0..*count).map(|span| span_name(&name, span)));
    }
    names.extend(["wall:1", "top", "wall:0"].map(String::from));
    names
}

/// The one body of `region` on `plane` turned by `angle` about the line through `at` along `dir`, with one face
/// for each curve whose spans meet at `joints` (where the kernel cannot make it so, a face for each span).
fn revolved(plane: &Plane, region: &Region2, at: Vec2, dir: Vec2, angle: f64, joints: &Joints) -> Result<Body> {
    let one_curve = |s: &Seg2, t: &Seg2| joints.joins(s, t);
    let whole = kernel::revolve_whole_sides(plane, std::slice::from_ref(region), at, dir, angle, &one_curve).ok();
    let mut bodies = match whole {
        Some(bodies) => bodies,
        None => kernel::revolve(plane, std::slice::from_ref(region), at, dir, angle)?,
    };
    match (bodies.pop(), bodies.is_empty()) {
        (Some(body), true) => Ok(body),
        _ => Err(DocError::Invalid("the rounded end of a tooth space is not one solid".into())),
    }
}

/// The hub's opening cut into a picked bore: filled in first when the bore is wider than the hub's teeth reach,
/// then one plug whose outline is the opening, cut from the bore's free end for the length. No rounded end.
fn cut_bore(st: &mut ModelState, n: &SplineNumbers, plan: &SplinePlan, seat: &Seat, warning: &mut Option<String>) -> Result<()> {
    let outline = &plan.hub;
    // The opening's smallest radius is the circle of the tips of the hub's teeth, its largest that of its spaces.
    let (minor, major) = (outline.least_radius, outline.most_radius);
    let length = n.length;
    check_room(seat, length, "bore")?;
    let tol = same_size(seat.radius);
    let margin = tool_margin(major - minor);
    // The two ends of the bore, as the hub has them before this feature: a chamfer the bore already has at an
    // entry is cut through, and follows the tops of the hub's teeth to their size. The far end matters where the
    // teeth run to it.
    let old_radius = seat.radius;
    let resized = (old_radius - minor).abs() > tol;
    let teeth_end = length >= seat.room - tol;
    let beyond = other_end(seat);
    let near = past_end(st, seat, true, major);
    let far = if teeth_end { past_end(st, &beyond, true, major) } else { shut_end(old_radius) };
    let seam = seam_angle(plan, true);
    // The spline's numbers win: a bore wider than the tips of the hub's teeth is first filled in to them.
    if seat.radius > minor + tol {
        // A bore that is a whole band is filled with a plain sleeve over the teeth. A bore with anything opening
        // in it is moved instead, so that what opens in it stays open.
        let moved = whole_band(st, seat) != Some(true);
        // Where the teeth stop short of the bore's far end, a moved bore is cut back to its own size from the end of
        // the teeth on: what lies past the bore's far end says how far that cut may run.
        let rest = if moved && !teeth_end { Some(past_end(st, &beyond, true, major)) } else { None };
        if moved {
            // The bore's own face moves in to the tops of the hub's teeth, and every face next to it is carried on
            // on its own surface: a set screw's hole or a keyway that opens in the bore stays open, with its own
            // walls, as if the bore had been that small when they were cut.
            move_round_face(st, seat, old_radius - minor, &format!("{}|moved", tool_key(n, seat))).map_err(|e| {
                DocError::Invalid(format!(
                    "the hub's teeth reach in to {} mm across, less than the bore's {} mm, and the bore's face, which has something opening in it (a set screw's hole, a keyway), could not be moved in that far ({e}): give the bore that diameter where it is made",
                    show_mm(2.0 * minor),
                    show_mm(2.0 * old_radius)
                ))
            })?;
        } else {
            // A bore that is a whole band: a plain sleeve in it over the teeth. (It would close what opens in a
            // bore: such a bore is moved instead, above.)
            let sleeve = Region2 { outer: Loop2::circle(Vec2::ZERO, seat.radius + margin), holes: vec![Loop2::circle(Vec2::ZERO, minor)] };
            let (from, to) = seat.span(0.0, length);
            let body = extruded(&seat.plane, &sleeve, from, to, &Joints::default(), "sleeve that fills the bore")?;
            // Its hole is the bore from now on: the tops of the hub's teeth.
            change(st, &seat.body, &built_round(body, &sleeve, "outside", "bore"), BoolOp::Union, "filling the bore")?;
        }
        for (e, at) in [(&near, seat), (&far, &beyond)] {
            if e.end > 0.0 && e.cone {
                // The bore's own entry (a chamfer, made at the old size) is filled with it, square, to the end
                // of the body; the chamfer is cut again below, at the new size. A moved face took the chamfer with
                // it along the chamfer's own cone, which meets the new bore further in: the ring reaches as far.
                let carried = if moved && e.rim > old_radius { end_carried(e, old_radius, minor) } else { 0.0 };
                let ring = Region2 { outer: Loop2::circle(Vec2::ZERO, e.rim + margin), holes: vec![Loop2::circle(Vec2::ZERO, minor)] };
                let (from, to) = at.span(-e.end, carried);
                let body = extruded(&at.plane, &ring, from, to, &Joints::default(), "ring that fills the bore's entry")?;
                change(st, &seat.body, &built_round(body, &ring, "outside", "bore"), BoolOp::Union, "filling the bore's entry")?;
            }
        }
        if let Some(e) = rest {
            // Past the teeth the bore keeps its own size: a round plug of the bore's own radius is cut out again
            // from the end of the teeth to the bore's far end, and a little past it where the bore opens out there.
            let past = if e.open { margin } else { 0.0 };
            let plug = Region2 { outer: Loop2::circle(Vec2::ZERO, old_radius), holes: Vec::new() };
            let (from, to) = seat.span(length, seat.room + past);
            let body = extruded(&seat.plane, &plug, from, to, &Joints::default(), "plug that keeps the rest of the bore")?;
            change(st, &seat.body, &built_round(body, &plug, "bore", "inner"), BoolOp::Cut, "keeping the rest of the bore")?;
        }
        // The filled bore is still the bore: it keeps the name the bore's own face had.
        also_from(st, &seat.body, "bore", seat.face);
    }
    // A plain chamfer the bore had at an entry keeps its size on the tops of the hub's teeth: cut again there.
    if resized {
        for (e, at, name) in [(&near, seat, "blank:chamfer"), (&far, &beyond, "blank:chamfer:far")] {
            if !(e.end > 0.0 && e.cone) {
                continue;
            }
            let chamfer = EndChamfer { radius: minor, radial: e.rim - old_radius, axial: e.end, past: e.end, flush: None };
            let Ok(section) = chamfer_section(&chamfer, margin, false) else { continue };
            change(st, &seat.body, &chamfer_tool(at, &section, seam, name)?, BoolOp::Cut, "the chamfer the bore had at its entry")?;
        }
    }
    // The spline's own chamfer at the bore's entry, cut on the round bore before the spaces: out from the tops of
    // the hub's teeth, at whatever size they now have. It goes on each end the teeth run out of: the free end,
    // and the far end where the teeth run the whole bore.
    if let Some(size) = plan.entry_chamfer {
        let keys = OwnChamfer::Entry.keys(n);
        let mut cut = false;
        for (e, at, name, at_far) in [(&near, seat, "chamfer", false), (&far, &beyond, "chamfer:far", true)] {
            if at_far && !teeth_end {
                continue;
            }
            let Some(chamfer) = own_chamfer(e, minor, size, false) else { continue };
            // At a step into a counterbore, a chamfer wider than the ledge would cut in under the wall.
            if chamfer.flush.is_some() && !beside_clear(st, at, minor + size.radial) {
                continue;
            }
            let section = chamfer_section(&chamfer, margin, false).map_err(|_| DocError::Invalid(keys.not_in_bore(minor)))?;
            change(st, &seat.body, &chamfer_tool(at, &section, seam, name)?, BoolOp::Cut, "the chamfer at the bore's entry")?;
            cut = true;
        }
        if !cut {
            append_warning(
                warning,
                "the entry chamfer was not cut: the teeth run out at no open end of the bore, or the chamfer is wider than the ledge there".into(),
            );
        }
    }
    let mut joints = Joints::default();
    let (opening, names) = outline_loop(outline, plan.hub_start, &mut joints, "tooth", count_of(n, plan))?;
    let plug = Region2 { outer: opening, holes: Vec::new() };
    // The plug starts past the bore's free end, and runs out through what the bore has at its entry; where the
    // teeth run the whole bore and out of an open far end, it runs out there too.
    let stop = if teeth_end && far.out { seat.room + end_run_out(&far) + margin } else { length };
    // At the bottom of a blind bore the plug starts with the face: nothing past it is cut into.
    let before = if near.open { -(end_run_out(&near) + margin) } else { 0.0 };
    let (from, to) = seat.span(before, stop);
    let body = extruded(&seat.plane, &plug, from, to, &joints, "plug that cuts the hub's teeth")?;
    change(st, &seat.body, &built(body, &|s| names.get(s).cloned(), ["start", "end"]), BoolOp::Cut, "the hub's spline")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reading of what stands past the end of the teeth that could not be made is refused in words that name the
    /// cutter and the way out: it is never taken as nothing standing there.
    #[test]
    fn an_unread_shoulder_past_the_teeth_is_refused_not_taken_as_none() {
        assert_eq!(cutter_way_out(Some(14.5)).ok(), Some(14.5));
        let words = match cutter_way_out(None) {
            Err(DocError::Invalid(words)) => words,
            other => panic!("expected a refusal, got {other:?}"),
        };
        assert!(words.starts_with("`cutter_diameter`"), "{words}");
        assert!(words.contains("could not be read") && words.contains("leave `cutter_diameter` out"), "{words}");
    }

    /// A refusal about how far one of the spline's own chamfers reaches across the end names the parameter that sets
    /// that reach: its second distance or its angle where one is given, else the chamfer itself.
    #[test]
    fn a_chamfer_refusal_names_what_sets_its_reach() {
        let n = SplineNumbers {
            end_chamfer: Some(1.0),
            end_chamfer_angle: Some(60.0),
            entry_chamfer: Some(1.0),
            entry_chamfer_distance2: Some(0.5),
            ..SplineNumbers::default()
        };
        assert_eq!(OwnChamfer::End.keys(&n).across, "end_chamfer_angle");
        assert_eq!(OwnChamfer::Entry.keys(&n).across, "entry_chamfer_distance2");
        assert_eq!(OwnChamfer::End.keys(&n).size, "end_chamfer");
        let plain = SplineNumbers { end_chamfer: Some(1.0), entry_chamfer: Some(1.0), ..SplineNumbers::default() };
        assert_eq!(OwnChamfer::End.keys(&plain).across, "end_chamfer");
        assert_eq!(OwnChamfer::Entry.keys(&plain).across, "entry_chamfer");
        let size = ChamferSize { axial: 1.0, radial: 1.7 };
        let words = OwnChamfer::End.keys(&n).reaches_the_axis(size, 0.9);
        assert!(words.starts_with("`end_chamfer_angle`") && words.contains("reach the axis"), "{words}");
        let words = OwnChamfer::End.keys(&plain).reaches_the_axis(size, 0.9);
        assert!(words.starts_with("`end_chamfer` must be less than"), "{words}");
        let words = OwnChamfer::Entry.keys(&n).not_in_bore(5.0);
        assert!(words.starts_with("`entry_chamfer_distance2` cannot be cut in a bore of"), "{words}");
    }
}
