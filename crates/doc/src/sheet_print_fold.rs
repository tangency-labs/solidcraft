//! Foldable 3D print: a hinged sheet ([`SheetBody::hinged`]) made as one flat body to print and
//! fold. Its outer skin lies on the print bed (z = 0 of the flat frame) and every bend turns
//! toward +z. A V-groove is cut down from the top at each bend's virtual sharp, its two walls at
//! half the bend angle each side of upright, so that once folded they meet as a mitre. Under the
//! groove a film `film` thick is left: the hinge the panels fold on. `clearance` moves each wall
//! of the groove that far apart from the other, measured square to the walls once folded.

use serde::{Deserialize, Serialize};

use super::{FlatWork, SheetBody, extrude_flat};
use crate::{DocError, Result};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{self as kernel, Body};

/// Values a foldable print takes, as checked by [`check_print_fold`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintFold {
    /// The film left under each V-groove (mm).
    pub film: f64,
    /// How far the folded walls of a groove stay apart (mm).
    pub clearance: f64,
}

/// The default film under a V-groove (mm): two or three printed layers.
pub const DEFAULT_FILM: f64 = 0.4;

/// The default clearance of a foldable print (mm): fingers that print side by side stay this far
/// apart, so they don't fuse (a PLA test print at 0.1 mm joined them a little).
pub const DEFAULT_PRINT_CLEARANCE: f64 = 0.15;

/// The narrowest a joint's finger is made (mm): an edge too short for the fingers asked for gets
/// fewer, at least two (a test print's 3 mm fingers were frail).
pub const MIN_FINGER_WIDTH: f64 = 5.0;

/// How many fingers a joint asked for `segments` gets on a stretch `len` long: no more than
/// leave each [`MIN_FINGER_WIDTH`] wide, and at least two.
pub fn finger_count(segments: u32, len: f64) -> u32 {
    let fit = (len / MIN_FINGER_WIDTH).floor();
    let fit = if fit.is_finite() && fit >= 2.0 { fit.min(f64::from(u32::MAX)) as u32 } else { 2 };
    segments.min(fit).max(2.min(segments.max(1)))
}

/// The least two panels of a foldable print that lie side by side in the flat stay apart where
/// their fingers lean toward each other (mm): a line of a 0.4 mm nozzle, so they print apart.
pub const FINGER_FLAT_GAP: f64 = 0.4;

/// A loose piece of a foldable print smaller than this is a crumb its cuts left, and is dropped (mm³).
pub const CRUMB: f64 = 5.0;

/// A corner fill smaller than this share of a cube of the wall's thickness is a fin, and is left out.
pub const FIN: f64 = 0.15;

/// The default wall round the channel or the nub of a joint on a bend, inside its fingers (mm):
/// two lines of a 0.4 mm nozzle.
pub const DEFAULT_JOINT_WALL: f64 = 0.8;

/// How a foldable print's panels meet where two of them touch along an edge that is no bend
/// (an edge cut open in the flat).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrintJoint {
    /// Square ends: the panel nearer the fixed face runs on to the corner, the other ends on its
    /// inside, the clearance short of it.
    #[default]
    Butt,
    /// Both ends cut on the plane halfway between the panels, each half the clearance short of
    /// it: their outer faces meet at the corner.
    Mitre,
}

impl PrintJoint {
    pub fn parse(s: &str) -> Option<PrintJoint> {
        match s.trim().to_ascii_lowercase().as_str() {
            "butt" => Some(PrintJoint::Butt),
            "mitre" | "miter" => Some(PrintJoint::Mitre),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            PrintJoint::Butt => "butt",
            PrintJoint::Mitre => "mitre",
        }
    }
}

/// The fingers a filament joint has by default along its edge.
pub const DEFAULT_SEGMENTS: u32 = 5;

/// The default filament diameter (mm).
pub const DEFAULT_FILAMENT: f64 = 1.75;

/// The default play of a filament channel (mm): test prints took the filament at no play, and in
/// places at 0.05 mm, only with a lot of force.
pub const DEFAULT_CHANNEL_TOLERANCE: f64 = 0.1;

/// A filament joint on an open edge of a foldable print, the edge picked at `at`: the edge
/// split evenly into `segments` fingers that belong to its two panels by turns, and a square
/// channel through all of them for a filament `diameter` thick, `tolerance` wider (negative:
/// narrower, for a press fit). The panel nearer the fixed face has the first finger at the
/// edge's first end, or the other one with `flip`; an odd number of fingers gives one panel
/// both ends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintFilament {
    pub at: Vec3,
    #[serde(default = "default_segments")]
    pub segments: u32,
    #[serde(default = "default_filament")]
    pub diameter: f64,
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// A press fit in place of the filament: the same fingers, no channel, a nub on each end
    /// face of the starting panel's fingers that snaps into a dimple in the next finger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub press: Option<PressFit>,
    /// On a bend: the wall round the channel or the nub inside the fingers (mm).
    #[serde(default = "default_wall")]
    pub wall: f64,
}

/// The default press-fit nub: its radius on the finger's end face and how far it sticks out (mm).
pub const DEFAULT_NUB_RADIUS: f64 = 0.8;
pub const DEFAULT_NUB_HEIGHT: f64 = 0.3;

/// A press fit's nub and dimple: a cap of a ball, `radius` round on the finger's end face and
/// `height` high; the dimple is the same ball `tolerance` larger (negative: tighter).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PressFit {
    pub radius: f64,
    pub height: f64,
    #[serde(default)]
    pub tolerance: f64,
}

impl PressFit {
    /// The radius of the ball the nub is a cap of.
    pub fn ball(&self) -> f64 {
        (self.radius * self.radius + self.height * self.height) / (2.0 * self.height)
    }

    /// The dimple's radius where it opens: the larger ball's cap beyond the nub's face.
    pub fn dimple_radius(&self) -> f64 {
        let (big, back) = (self.ball() + self.tolerance, self.ball() - self.height);
        (big * big - back * back).max(0.0).sqrt()
    }

    /// Refuse a nub that can't be made or can't snap: `clearance` is the gap between the
    /// fingers, `room` how far from the middle of the wall (`t` thick) the nub may reach.
    pub fn check(&self, clearance: f64, room: f64, t: f64) -> std::result::Result<(), String> {
        if !(self.radius.is_finite() && self.radius > 0.0 && self.radius <= 20.0) {
            return Err("the nub's radius must be more than 0 and at most 20 mm".into());
        }
        if !(self.height.is_finite() && self.height > 0.0 && self.height <= self.radius) {
            return Err("the nub's height must be more than 0 and at most its radius".into());
        }
        if !(self.height > clearance) {
            return Err(format!(
                "the nub sticks out {:.2} mm, no more than the {clearance:.2} mm clearance between the fingers: it can't reach the dimple",
                self.height
            ));
        }
        if !(self.tolerance.is_finite() && self.tolerance > -self.height) {
            return Err("the dimple's tolerance must leave a dimple".into());
        }
        if !(self.radius.max(self.dimple_radius()) <= room) {
            return Err(format!(
                "a nub {:.2} mm round needs walls at least {:.2} mm thick (the nub, and the film either side)",
                self.radius,
                t + 2.0 * (self.radius - room)
            ));
        }
        Ok(())
    }
}

fn default_segments() -> u32 {
    DEFAULT_SEGMENTS
}

fn default_filament() -> f64 {
    DEFAULT_FILAMENT
}

fn default_tolerance() -> f64 {
    DEFAULT_CHANNEL_TOLERANCE
}

fn default_wall() -> f64 {
    DEFAULT_JOINT_WALL
}

impl PrintFilament {
    /// The square channel's side.
    pub fn side(&self) -> f64 {
        self.diameter + self.tolerance
    }
}

/// Sockets along a straight edge of a hole through a face of a foldable print, the edge from
/// `a` to `b` (the body's coordinates): the hole's wall there is printed as a part of its own
/// (a carved-out group of Unfold for Printing), and fingers of it stand in the face along
/// `fingers` (stretches from `a`, mm). The face is notched for each, a wall and the clearance
/// deep, with a dimple for a press-fit nub in both sides of each notch, or with `filament` a
/// channel along the edge through what is left of the face there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintSocket {
    pub a: Vec3,
    pub b: Vec3,
    pub fingers: Vec<(f64, f64)>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub filament: bool,
}

/// Where a hole wall `len` long has its fingers in the face it meets (from the edge's first end):
/// every other stretch of the edge split as a joint's is, never the first or the last, so the
/// corners of the hole stay the face's. An edge too short for three stretches of
/// [`MIN_FINGER_WIDTH`] gets three narrower ones from 9 mm on, and none below.
pub fn socket_fingers(len: f64) -> Vec<(f64, f64)> {
    if !(len.is_finite() && len >= 9.0) {
        return Vec::new();
    }
    let n = finger_count(DEFAULT_SEGMENTS, len).max(3);
    let each = len / f64::from(n);
    (0..n).filter(|k| k % 2 == 1 && k + 1 < n).map(|k| (f64::from(k) * each, f64::from(k + 1) * each)).collect()
}

/// A filament joint's cuts in one panel of the flat: the open edge from `a` to `b` on the
/// outer skin as the face drew it, `n` its unit direction into the panel. `notches` are the
/// stretches along the edge (from `a`, mm) where the other panel's fingers are: this panel ends
/// `depth` in from the edge there. The channel runs along the whole edge, `offset` in from it
/// at mid-thickness, square, `side` wide.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilamentCut {
    pub a: Vec2,
    pub b: Vec2,
    pub n: Vec2,
    pub notches: Vec<(f64, f64)>,
    pub depth: f64,
    pub offset: f64,
    pub side: f64,
    /// A press fit in place of the channel, and whether this panel's fingers carry the nubs (else
    /// the dimples).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub press: Option<PressFit>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nubs: bool,
    /// The fold between this panel and the other (radians, 0 where both lie flat on each other):
    /// under 90° the corner both walls share reaches past this panel's edge, and its fingers take
    /// that part too.
    #[serde(default)]
    pub angle: f64,
    /// The panel this edge is on (its flange; none for the base).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<usize>,
}

impl FilamentCut {
    /// The stretches along the edge (from `a`, mm) where this panel's own fingers are.
    pub fn own_fingers(&self) -> Vec<(f64, f64)> {
        let len = (self.b - self.a).len();
        let mut notches = self.notches.clone();
        notches.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut out = Vec::new();
        let mut at = 0.0_f64;
        for (lo, hi) in notches {
            if lo > at + 1e-9 {
                out.push((at, lo.min(len)));
            }
            at = at.max(hi);
        }
        if at < len - 1e-9 {
            out.push((at, len));
        }
        out.retain(|(s, e)| e - s > 1e-9);
        out
    }

    /// How far past the edge (on its inner skin) this panel's fingers reach, the corner both
    /// walls share out to the other panel's outer face: `t` over the tangent of the fold.
    pub fn reach(&self, t: f64) -> f64 {
        if self.angle > 1e-6 && self.angle < std::f64::consts::FRAC_PI_2 - 1e-6 { t / self.angle.tan() } else { 0.0 }
    }
}

/// A filament joint on a bend of a foldable print (flange `flange`'s): the bend split evenly into
/// `segments` fingers along its line, each giving the folded corner to one of its two panels by
/// turns (the parent first, or the flange with `flip`), the film under them left whole. The
/// channel, square and `side` wide, runs along the bend on its core ([`fold_core`]), `wall`
/// inside the fingers round it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FoldFilament {
    pub flange: usize,
    pub segments: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    pub side: f64,
    /// A press fit in place of the channel (the starting panel's fingers carry the nubs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub press: Option<PressFit>,
    #[serde(default = "default_wall")]
    pub wall: f64,
    /// Where another face of the print meets an end of the bend.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ends: Vec<FoldEnd>,
}

impl FoldFilament {
    /// The joint's core on a bend folded by `angle` in a wall `t` thick.
    pub fn core(&self, t: f64, angle: f64, pf: PrintFold) -> std::result::Result<FoldCore, String> {
        match self.press {
            Some(fit) => fold_core(t, angle, pf, fit.radius.max(fit.dimple_radius()), true, self.wall),
            None => fold_core(t, angle, pf, self.side / 2.0, false, self.wall),
        }
    }
}

/// Another face of a foldable print at an end of a joined bend: the end `u` along the bend (from
/// its flange's `p` along `d`, mm), the sine of the angle between the bend's line and that face,
/// and the flange whose bend is joined there too, if one is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FoldEnd {
    pub u: f64,
    pub sin: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joined: Option<usize>,
}

/// Where a joint on a bend runs: its core `d` from the bend's sharp (on the outer skin) along
/// the bisector of the fold, inside it; its fingers a sector `r` round the sharp. A press fit's
/// nub sits at `nub` on the parent's fingers (across the bend from the sharp toward the flange,
/// and up from the outer skin): on the core, or where the core is no place for it, nearer the
/// parent's wall ([`fold_core`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoldCore {
    pub d: f64,
    pub r: f64,
    pub nub: Vec2,
}

/// The fold a joint on a bend takes (degrees).
pub const FOLD_JOINT_ANGLES: (f64, f64) = (30.0, 150.0);

/// The core of a joint on a bend folded by `angle` in a wall `t` thick: a channel (square,
/// `half` its side each way, lined up with the wall of the finger it runs in) or a nub (`round`,
/// `half` its radius), `wall` from the panel's outer skin and, with the film and the clearance
/// besides, from the other panel's. The fingers are a sector round the sharp large enough for
/// the core with its wall and for the corner where the two walls overlap once folded.
pub fn fold_core(t: f64, angle: f64, pf: PrintFold, half: f64, round: bool, wall: f64) -> std::result::Result<FoldCore, String> {
    let angle = angle.abs();
    let (lo, hi) = FOLD_JOINT_ANGLES;
    if !(angle.is_finite() && angle >= lo.to_radians() - 1e-6 && angle <= hi.to_radians() + 1e-6) {
        return Err(format!("it needs a fold of {lo}° to {hi}°, not {:.1}°", angle.to_degrees()));
    }
    if !(wall.is_finite() && wall > 0.0 && wall <= 20.0) {
        return Err("the wall round its channel or nub must be more than 0 and at most 20 mm".into());
    }
    if !(half.is_finite() && half > 0.0 && half <= 20.0) {
        return Err("its channel or nub must be more than 0 and at most 40 mm across".into());
    }
    let (sin, cos) = (angle.sin(), angle.cos());
    let (s2, c2) = ((angle / 2.0).sin(), (angle / 2.0).cos());
    let m = pf.film + pf.clearance;
    // How far the core's outline reaches toward the other panel's outer skin.
    let reach = if round { half } else { half * (sin + cos.abs()) };
    let d = ((reach + m + wall) / c2).max((half + wall) / c2);
    let far = if round { d + half } else { (d * s2 + half).hypot(d * c2 + half) };
    // The corner both walls fill once folded, and the line beside the other's film at the inner skin.
    let keep_top = (t * cos - m) / sin;
    let corner = (t / c2).max(t / sin).max(keep_top.hypot(t));
    let over = (t * 0.05).max(1e-3);
    let r = (far + wall).max(corner + 0.05).max(t + over + 0.05);
    if !(d.is_finite() && r.is_finite() && r <= 100.0) {
        return Err("its fingers would reach more than 100 mm round the bend".into());
    }
    // A nub stands out of its finger's end face into the next finger's stretch of the bend. In
    // the flat that finger is the other panel's, mirrored across the sharp: it lies where
    // `x·sin + z·cos` is at least `m`, and the nub must print clear of it. At a small fold it
    // covers the core; the nub then goes as low as its wall lets it once folded (the other
    // panel's finger starts `m` above this one's outer skin), and as far toward this panel's
    // wall as clears the other's finger in the flat.
    let core = Vec2::new(-d * s2, d * c2);
    let gap = pf.clearance.max(0.05);
    let clear_of_mirror = |p: Vec2| p.x * sin + p.y * cos <= m - half - gap + 1e-9;
    let nub = if !round || clear_of_mirror(core) {
        core
    } else {
        let z = m + half + wall;
        let low = Vec2::new((m - half - gap - z * cos) / sin, z);
        if !(low.x * sin - low.y * cos <= -m - half) {
            return Err(format!("at a fold of {:.1}° its fingers have no room for a nub clear of the next finger in the flat", angle.to_degrees()));
        }
        low
    };
    // The fingers reach round the nub with its wall too.
    let r = r.max(nub.len() + half + wall);
    if !(r <= 100.0) {
        return Err("its fingers would reach more than 100 mm round the bend".into());
    }
    Ok(FoldCore { d, r, nub })
}

/// A corner of a folded print where a bend ends against other faces: the corner `v` (folded,
/// in the flat's frame), each panel whose outer face runs through it (none: the base) with that
/// face's outward normal, and each bend that ends there: its flange, where along it from `p` its
/// zone ends, and whether the corner lies past that end (else before the zone's start).
struct PrintCorner {
    v: Vec3,
    faces: Vec<(Option<usize>, Vec3)>,
    bends: Vec<(usize, f64, bool)>,
}

/// Distance from `p` to a polygon (0 inside it).
fn dist_to_polygon(poly: &[Vec2], p: Vec2) -> f64 {
    if inside(p, poly) {
        return 0.0;
    }
    edges(poly).map(|(a, b)| seg_dist(p, a, b)).fold(f64::INFINITY, f64::min)
}

/// Whether a polygon comes within `by` of the box `lo`…`hi` (or overlaps it).
fn touches_box(poly: &[Vec2], lo: Vec2, hi: Vec2, by: f64) -> bool {
    let (lo, hi) = (Vec2::new(lo.x - by, lo.y - by), Vec2::new(hi.x + by, hi.y + by));
    let in_box = |p: Vec2| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y;
    let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
    if poly.iter().any(|p| in_box(*p)) || corners.iter().any(|c| inside(*c, poly)) {
        return true;
    }
    // An edge of the polygon across the box: it crosses one of the box's edges.
    let cross = |a: Vec2, b: Vec2, c: Vec2, d: Vec2| {
        let side = |p: Vec2, q: Vec2, r: Vec2| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
        side(a, b, c) * side(a, b, d) < 0.0 && side(c, d, a) * side(c, d, b) < 0.0
    };
    edges(poly).any(|(a, b)| (0..4).any(|k| cross(a, b, corners[k], corners[(k + 1) % 4])))
}

/// Where a joint's fingers are along one stretch of a bend ([`SheetBody::fold_layout`]).
struct FoldLayout {
    core: FoldCore,
    a0: f64,
    a1: f64,
    len: f64,
    k: u32,
    each: f64,
}

/// A mitred end of a panel in the flat: the edge from `a` to `b` on the outer skin (z = 0, as the
/// face drew it), `n` its unit direction into the panel. The panel's end is square at `at` in
/// from the edge; the mitre takes it `extra` further in at the inner skin (z = T), in a straight
/// line between.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bevel {
    pub a: Vec2,
    pub b: Vec2,
    pub n: Vec2,
    pub at: f64,
    pub extra: f64,
}

/// How far a groove's half-width grows at every height for `clearance`: the walls lean by half
/// the bend angle, so a shift square to a wall is 1/cos of it across the flat.
fn clearance_shift(clearance: f64, angle: f64) -> f64 {
    clearance / 2.0 / (angle.abs() / 2.0).cos()
}

/// Refuse values a foldable print can't be made with, with a message that says which and why.
pub fn check_print_fold(sh: &SheetBody, pf: PrintFold) -> Result<()> {
    let t = sh.t;
    if !(pf.film.is_finite() && pf.film > 0.0 && pf.film < t) {
        return Err(DocError::Invalid(format!("the film thickness must be more than 0 and less than the wall thickness ({t:.3} mm)")));
    }
    if !(pf.clearance.is_finite() && pf.clearance >= 0.0) {
        return Err(DocError::Invalid("the clearance must not be negative".into()));
    }
    if !sh.hinged {
        return Err(DocError::Invalid("a foldable print needs at least one bend".into()));
    }
    for (i, f) in sh.flanges.iter().enumerate() {
        // The groove's widening by the clearance reaches into the panel past the bend; that panel
        // keeps some material only if its leg is longer than its own groove's widening plus the
        // widest one of the bends on it.
        let own = clearance_shift(pf.clearance, f.angle);
        let kids = sh.flanges.iter().filter(|g| g.parent == Some(i)).map(|g| clearance_shift(pf.clearance, g.angle)).fold(0.0, f64::max);
        if !(own.is_finite() && own + kids < f.leg) {
            return Err(DocError::Invalid(format!(
                "the clearance makes the V-groove at bend {} wider than the panel past it; make the clearance smaller",
                i + 1
            )));
        }
    }
    Ok(())
}

/// The gap between two parts of a split foldable print on the bed (mm).
pub const PART_GAP: f64 = 5.0;

/// The flat bodies of a split foldable print's parts as one body, placed in the first part's
/// frame: the first where its sheet lies, every other one beside it in a row along the first's
/// flat y, [`PART_GAP`] (or two walls, if more) apart, all lined up on the first's least x.
pub fn print_fold_parts(parts: &[SheetBody], pf: PrintFold) -> Result<Body> {
    let Some(first) = parts.first() else { return Err(DocError::Invalid("a foldable print needs at least one part".into())) };
    if parts.iter().all(|p| p.flanges.is_empty()) {
        return Err(DocError::Invalid("a foldable print needs at least one bend".into()));
    }
    let mut body = first.print_fold_local(pf)?;
    let (first_lo, first_hi) = first.flat_box()?;
    let gap = PART_GAP.max(2.0 * first.t);
    let mut y = first_hi.y;
    for sh in parts.iter().skip(1) {
        let (lo, hi) = sh.flat_box()?;
        let part = sh.print_fold_local(pf)?;
        let shift = Vec3::new(first_lo.x - lo.x, y + gap - lo.y, 0.0);
        let part = kernel::transform(&part, shift, Vec3::new(0.0, 0.0, 0.0), Vec3::Z, 0.0)?;
        body = kernel::boolean(&body, &part, kernel::BoolOp::Union)?
            .ok_or_else(|| DocError::Invalid("the parts of the foldable print leave nothing".into()))?;
        y += gap + (hi.y - lo.y);
    }
    first.place_local_solid(body)
}

/// A foldable print's parts folded back up, each where its faces were: the shape the print
/// takes once folded, its fingers and channels as printed.
pub fn print_folded_parts(parts: &[SheetBody], pf: PrintFold) -> Result<Body> {
    let mut pieces = Vec::with_capacity(parts.len());
    for sh in parts {
        pieces.push(sh.print_folded_solid(pf)?);
    }
    joined(pieces)
}

/// Pieces that meet face to face as one body: their union, or where the kernel can't make one
/// (faces that lie on each other), the pieces side by side in one body.
fn joined(pieces: Vec<Body>) -> Result<Body> {
    if pieces.len() < 2 {
        return pieces.into_iter().next().ok_or_else(|| DocError::Invalid("the foldable print has no pieces".into()));
    }
    let refs: Vec<&Body> = pieces.iter().collect();
    if let Ok(body) = kernel::union_all(&refs) {
        return Ok(body);
    }
    kernel::join_apart(&pieces)?.ok_or_else(|| DocError::Invalid("the foldable print's pieces can't be joined".into()))
}

impl SheetBody {
    /// A foldable print folded back up, in the sheet's frame. Each bend turns about the middle of
    /// its film on the outer skin, so the flat print is cut there: each panel with the halves of
    /// the bend zones beside it turns as its bend folds it, and the groove's walls close.
    pub fn print_folded_solid(&self, pf: PrintFold) -> Result<Body> {
        let (flat, fills) = self.print_fold_local_fills(pf)?;
        let t = self.t;
        let over = (t * 0.05).max(1e-3);
        // A joined bend's fingers stand out of the walls, and reach past the sharp: those parts
        // go with the panel whose finger it is.
        let mut top = t + over;
        let mut moves: Vec<(usize, bool, Body)> = Vec::new();
        for b in self.bends() {
            let Some(f) = b.index.checked_sub(1).and_then(|i| self.flanges.get(i)) else { continue };
            let Some(ff) = self.fold_filament.iter().find(|ff| ff.flange + 1 == b.index) else { continue };
            if let Ok(core) = ff.core(t, f.angle, pf) {
                top = top.max(core.r + pf.clearance + over);
            }
            for (parent, tool) in self.fold_moves(b.start, b.end, f, ff, pf)? {
                moves.push((ff.flange, parent, tool));
            }
        }
        let prism = |r: Region2| extrude_flat(&Plane::XY, &[r], -over, top);
        // Which step of folding a panel back failed, and for which panel.
        let step = |what: &str, owner: Option<usize>, e: kernel::KernelError| {
            let panel = owner.map_or_else(|| "the base".to_string(), |i| format!("the panel of bend {}", i + 1));
            DocError::Invalid(format!("folding the print back up ({what}, {panel}): {e}"))
        };
        let mut pieces = Vec::with_capacity(self.flanges.len() + 1);
        for owner in std::iter::once(None).chain((0..self.flanges.len()).map(Some)) {
            let mut tools = Vec::new();
            for r in self.piece_regions(owner) {
                tools.push(prism(r)?);
            }
            let refs: Vec<&Body> = tools.iter().collect();
            let mut tool = kernel::union_all(&refs).map_err(|e| step("its regions", owner, e))?;
            // An open edge's fingers past their panel's edge go with that panel. Each strip stops
            // a little short of the tool's top and bottom, at its own height: faces lying on
            // each other are where the kernel's booleans fail.
            let mut k = 0usize;
            for fc in self.filament.iter().filter(|fc| fc.panel == owner) {
                let reach = fc.reach(t);
                let Some(d) = (fc.b - fc.a).normalized() else { continue };
                if !(reach > 1e-9) {
                    continue;
                }
                for (s, e) in fc.own_fingers() {
                    let (a, b) = (fc.a + d * s, fc.a + d * e);
                    let (inside, outside) = (fc.n * 0.01, fc.n * -(reach + 0.01));
                    let step_z = over * 0.05 * (k % 8) as f64;
                    k += 1;
                    let strip = extrude_flat(
                        &Plane::XY,
                        &[super::rect(a + inside, b + inside, b + outside, a + outside)],
                        -over * 0.6 + step_z,
                        top - over * 0.4 - step_z,
                    )?;
                    tool =
                        kernel::boolean(&tool, &strip, kernel::BoolOp::Union).map_err(|e| step("an open edge's fingers", owner, e))?.unwrap_or(tool);
                }
            }
            // A corner's fills go with their panels.
            for (fill_owner, fill) in &fills {
                let op = if *fill_owner == owner { kernel::BoolOp::Union } else { kernel::BoolOp::Cut };
                if let Some(x) = kernel::boolean(&tool, fill, op).map_err(|e| step("a corner's fill", owner, e))? {
                    tool = x;
                }
            }
            for (flange, parent, part) in &moves {
                let Some(g) = self.flanges.get(*flange) else { continue };
                let (gains, loses) = if owner == g.parent {
                    (*parent, !*parent)
                } else if owner == Some(*flange) {
                    (!*parent, *parent)
                } else {
                    (false, false)
                };
                if gains {
                    tool = kernel::boolean(&tool, part, kernel::BoolOp::Union).map_err(|e| step("a finger past a sharp", owner, e))?.unwrap_or(tool);
                } else if loses {
                    tool = match kernel::boolean(&tool, part, kernel::BoolOp::Cut).map_err(|e| step("a finger past a sharp", owner, e))? {
                        Some(x) => x,
                        None => continue,
                    };
                }
            }
            let Some(piece) = kernel::boolean(&flat, &tool, kernel::BoolOp::Intersect).map_err(|e| step("its piece of the flat", owner, e))? else {
                continue;
            };
            pieces.push(match owner {
                Some(i) => kernel::transform_matrix(&piece, self.fold(i))?,
                None => piece,
            });
        }
        self.place_local_solid(joined(pieces)?)
    }

    /// The flat regions that fold as one piece with panel `owner` (None: the base): the panel and
    /// the halves of the bend zones beside it, up to the middle of each film.
    pub fn piece_regions(&self, owner: Option<usize>) -> Vec<Region2> {
        let half = |f: &super::SheetFlange, far: bool| {
            let ba = self.allowance(f);
            let (u0, u1) = self.zone_span(f);
            let (a, b) = (f.p + f.d * u0, f.p + f.d * u1);
            let (s, e) = if far { (ba / 2.0, ba) } else { (0.0, ba / 2.0) };
            super::rect(a + f.n * s, b + f.n * s, b + f.n * e, a + f.n * e)
        };
        let mut out = vec![match owner.and_then(|i| Some((i, self.flanges.get(i)?))) {
            None => Region2 { outer: self.base.outer.ccw(), holes: self.base.holes.clone() },
            Some((i, f)) => self.panel_with_holes(i, f),
        }];
        if let Some(f) = owner.and_then(|i| self.flanges.get(i))
            && self.allowance(f) > 1e-9
        {
            out.push(half(f, true));
        }
        for c in self.flanges.iter().filter(|c| c.parent == owner && self.allowance(c) > 1e-9) {
            out.push(half(c, false));
        }
        out
    }

    /// Where a joint on the bend from `start` to `end` (flange `f`'s) has its fingers, along the
    /// bend from `start`: its core ([`FoldFilament::core`]), and the stretch `a0`…`a1` the
    /// fingers share, short of every other face of the print that meets an end of the bend: the
    /// face's wall (or its own joint's fingers) and the clearance, more where it meets the bend
    /// askew.
    fn fold_layout(&self, start: Vec2, end: Vec2, f: &super::SheetFlange, ff: &FoldFilament, pf: PrintFold) -> Result<Option<FoldLayout>> {
        let len = (end - start).len();
        if !(len > 1e-9) {
            return Ok(None);
        }
        let t = self.t;
        let core = ff.core(t, f.angle, pf).map_err(|why| DocError::Invalid(format!("a joint on a bend: {why}")))?;
        let (u_s, u_e) = ((start - f.p).dot(f.d), (end - f.p).dot(f.d));
        // Along the bend from its flange's `p`: the fingers keep clear of each face at an end of the
        // edge (the bend itself may stop short of the edge's end, a wall's thickness say).
        let (mut lo, mut hi) = (u_s, u_e);
        for e in &ff.ends {
            let sin = e.sin.clamp(0.1, 1.0);
            let other = e
                .joined
                .and_then(|j| {
                    let g = self.flanges.get(j)?;
                    let fj = self.fold_filament.iter().find(|x| x.flange == j)?;
                    fj.core(t, g.angle, pf).ok()
                })
                .map_or(t, |c| c.r.max(t));
            let trim = (other + pf.clearance) / sin + core.r * (1.0 - sin * sin).max(0.0).sqrt() / sin;
            if e.u <= (u_s + u_e) / 2.0 {
                lo = lo.max(e.u + trim);
            } else {
                hi = hi.min(e.u - trim);
            }
        }
        let (a0, a1) = ((lo - u_s).max(0.0), (hi - u_s).min(len));
        let k = finger_count(ff.segments.max(1), a1 - a0);
        let each = (a1 - a0) / f64::from(k);
        if !(each - pf.clearance >= 1.0) {
            return Err(DocError::Invalid(format!(
                "a joint on a bend {len:.2} mm long, its fingers {:.2} mm round the bend and kept clear of the faces at its ends, leaves {k} fingers {:.2} mm each; take fewer fingers",
                core.r,
                each - pf.clearance
            )));
        }
        Ok(Some(FoldLayout { core, a0, a1, len, k, each }))
    }

    /// A V-groove along the bend from `start` (unit direction `d`, flange `f`'s), from `u0` to
    /// `u1` along it.
    #[allow(clippy::too_many_arguments)]
    fn v_groove(
        &self,
        body: Body,
        start: Vec2,
        d: Vec2,
        f: &super::SheetFlange,
        pf: PrintFold,
        over: f64,
        u0: f64,
        u1: f64,
        index: usize,
    ) -> Result<Body> {
        let t = self.t;
        let half = (f.angle.abs() / 2.0).tan();
        let shift = clearance_shift(pf.clearance, f.angle);
        let top = t + over;
        let (low, high) = (pf.film * half + shift, top * half + shift);
        let profile = Loop2::polygon(&[Vec2::new(-low, pf.film), Vec2::new(low, pf.film), Vec2::new(high, top), Vec2::new(-high, top)]);
        let mid = start + d * ((u0 + u1) / 2.0);
        let plane = Plane { origin: Vec3::new(mid.x, mid.y, 0.0), x: Vec3::new(d.y, -d.x, 0.0), y: Vec3::Z };
        let reach = (u1 - u0) / 2.0;
        let tool = extrude_flat(&plane, &[Region2 { outer: profile.ccw(), holes: Vec::new() }], -reach, reach)?;
        kernel::boolean(&body, &tool, kernel::BoolOp::Cut)?
            .ok_or_else(|| DocError::Invalid(format!("the V-groove at bend {index} leaves nothing of the print")))
    }

    /// A joint on a bend (filament or press fit), cut in the flat in place of its V-groove. Along
    /// the bend from `start` to `end` the fingers take turns between its two panels. Each is a
    /// sector of a circle round the bend's sharp on the outer skin, the line the bend folds about:
    /// from its own panel's outer skin round to the film and the clearance short of the other's,
    /// as far out as its channel or nub with its wall needs ([`fold_core`]), so inside the fold it
    /// stands out of the walls. The other panel is cut back to the clearance round it, down to its
    /// film, which stays whole. Folding turns the panels about that sharp, so neither passes
    /// through the other's fingers on the way. Where another face of the print meets an end of
    /// the bend, the fingers stop short of it and the bend is a V-groove there, the channel on
    /// through it. A channel is square, lined up with the wall of the finger it runs in: the
    /// round filament runs through the squares of both panels.
    #[allow(clippy::too_many_arguments)]
    fn fold_fingers(
        &self,
        mut body: Body,
        start: Vec2,
        end: Vec2,
        f: &super::SheetFlange,
        ff: &FoldFilament,
        pf: PrintFold,
        over: f64,
        index: usize,
        channels: &mut Vec<Body>,
    ) -> Result<Body> {
        use std::f64::consts::PI;
        let t = self.t;
        let Some(lay) = self.fold_layout(start, end, f, ff, pf)? else { return Ok(body) };
        let Some(d) = (end - start).normalized() else { return Ok(body) };
        let FoldCore { d: core, r, .. } = lay.core;
        let angle = f.angle.abs();
        let (sin, cos) = (angle.sin(), angle.cos());
        let (s2, c2) = ((angle / 2.0).sin(), (angle / 2.0).cos());
        let (film, clear) = (pf.film, pf.clearance);
        let m = film + clear;
        let rho = r + clear;
        let top = t + over;
        let (len, a0, a1, k, each) = (lay.len, lay.a0, lay.a1, lay.k, lay.each);
        let zc = core * c2;
        let pol = |rad: f64, a: f64| Vec2::new(rad * a.cos(), rad * a.sin());
        let arc = |rad: f64, a0: f64, a1: f64| Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: rad, start: a0, sweep: a1 - a0 };
        let line = |a: Vec2, b: Vec2| Seg2::Line { a, b };
        // Across the bend (from its sharp, toward the flange) and up, extruded along it.
        let tool_at = |lp: &Loop2, u0: f64, u1: f64| {
            let mid = start + d * ((u0 + u1) / 2.0);
            let plane = Plane { origin: Vec3::new(mid.x, mid.y, 0.0), x: Vec3::new(f.n.x, f.n.y, 0.0), y: Vec3::Z };
            let half = (u1 - u0) / 2.0;
            extrude_flat(&plane, &[Region2 { outer: lp.ccw(), holes: Vec::new() }], -half, half)
        };
        let cut = |body: Body, tool: Body| -> Result<Body> {
            kernel::boolean(&body, &tool, kernel::BoolOp::Cut)?
                .ok_or_else(|| DocError::Invalid("a joint on a bend leaves nothing of the print".into()))
        };
        // The flange's finger is the parent's mirrored across the sharp.
        let mirror = |lp: &Loop2| Loop2 {
            segs: lp
                .segs
                .iter()
                .map(|s| match s {
                    Seg2::Line { a, b } => Seg2::Line { a: Vec2::new(-a.x, a.y), b: Vec2::new(-b.x, b.y) },
                    Seg2::Arc { center, radius, start, sweep } => {
                        Seg2::Arc { center: Vec2::new(-center.x, center.y), radius: *radius, start: PI - *start, sweep: -*sweep }
                    }
                    other => *other,
                })
                .collect(),
        };
        // The dome every finger is cut from (both sides of the sharp, over the film's lower half so
        // none of its faces lies on the sheet's).
        let lo_dome = (film / 2.0 / r).asin();
        let dome = Loop2 { segs: vec![line(pol(r, PI - lo_dome), pol(r, lo_dome)), arc(r, lo_dome, PI - lo_dome)] };
        // A parent's finger: what lies within the film and the clearance of the flange's outer
        // skin once folded goes (the line through `q0` and `q2`), and the flange's wall the
        // clearance round the finger.
        let q0 = Vec2::new((film * cos - m) / sin, film);
        let lam = (rho * rho - m * m).max(0.0).sqrt();
        let q2 = Vec2::new(-m * sin + lam * cos, m * cos + lam * sin);
        let (b1, b2) = ((film / rho).asin(), q2.y.atan2(q2.x));
        if !(b2 > b1 + 1e-9 && r > top) {
            return Err(DocError::Invalid(format!("the joint on bend {index} has no room for its fingers")));
        }
        let skin = Loop2 { segs: vec![line(q0, pol(rho, b1)), arc(rho, b1, b2), line(q2, q0)] };
        let band = (clear > 1e-9).then(|| {
            let (af, at, bt) = ((film / r).asin(), (top / r).asin(), (top / rho).asin());
            Loop2 { segs: vec![line(pol(r, af), pol(rho, b1)), arc(rho, b1, bt), line(pol(rho, bt), pol(r, at)), arc(r, at, af)] }
        });
        // Between two fingers: nothing above the film within the clearance round them.
        let gap = Loop2 { segs: vec![line(pol(rho, PI - b1), pol(rho, b1)), arc(rho, b1, PI - b1)] };
        let square = |x: f64| {
            let h = ff.side / 2.0;
            Loop2::polygon(&[Vec2::new(x - h, zc - h), Vec2::new(x + h, zc - h), Vec2::new(x + h, zc + h), Vec2::new(x - h, zc + h)])
        };
        let (parent_sq, flange_sq) = (square(-core * s2), square(core * s2));
        let filament = ff.press.is_none();
        // The ends another face meets: a V-groove, the channel on through both panels.
        for (u0, u1) in [(-over, a0), (a1, len + over)] {
            if u1 - u0 <= over + 1e-9 {
                continue;
            }
            body = self.v_groove(body, start, d, f, pf, over, u0, u1, index)?;
            if filament {
                for sq in [&parent_sq, &flange_sq] {
                    channels.push(tool_at(sq, u0, u1)?);
                }
            }
        }
        let eps = (over * 0.2).min(0.01);
        body = kernel::boolean(&body, &tool_at(&dome, a0 + eps, a1 - eps)?, kernel::BoolOp::Union)?
            .ok_or_else(|| DocError::Invalid("a joint on a bend leaves nothing of the print".into()))?;
        let open_start = a0 <= 1e-9;
        let open_end = a1 >= len - 1e-9;
        for i in 0..k {
            let parent = (i % 2 == 0) != ff.flip;
            let (s, e) = (a0 + f64::from(i) * each, a0 + f64::from(i + 1) * each);
            let u0 = if i == 0 && open_start { -over } else { s - clear / 2.0 };
            let u1 = if i + 1 == k && open_end { len + over } else { e + clear / 2.0 };
            let side = |lp: &Loop2| if parent { lp.clone() } else { mirror(lp) };
            body = cut(body, tool_at(&side(&skin), u0, u1)?)?;
            if let Some(b) = &band {
                body = cut(body, tool_at(&side(b), u0, u1)?)?;
            }
            if filament {
                let q0 = if i == 0 && open_start { -over } else { s - clear / 4.0 };
                let q1 = if i + 1 == k && open_end { len + over } else { e + clear / 4.0 };
                channels.push(tool_at(if parent { &parent_sq } else { &flange_sq }, q0, q1)?);
            }
        }
        if clear > 1e-9 {
            for i in 0..=k {
                if (i == 0 && open_start) || (i == k && open_end) {
                    continue;
                }
                let b = a0 + f64::from(i) * each;
                body = cut(body, tool_at(&gap, b - clear / 2.0, b + clear / 2.0)?)?;
            }
        }
        let Some(fit) = ff.press else { return Ok(body) };
        // Each finger of the starting panel has a nub on its end faces ([`FoldCore::nub`]); the
        // next finger the same ball, a little larger, as a dimple where that nub lands once
        // folded: the nub's place turned back by the fold, in the other panel's flat.
        fit.check(clear, f64::INFINITY, t).map_err(|why| DocError::Invalid(format!("a press fit on a bend: {why}")))?;
        let (ball, back) = (fit.ball(), fit.ball() - fit.height);
        let at = |u: f64, p: Vec2| {
            let q = start + d * u + f.n * p.x;
            Vec3::new(q.x, q.y, p.y)
        };
        let p = lay.core.nub;
        let q = Vec2::new(p.x * cos + p.y * sin, -p.x * sin + p.y * cos);
        let (nub_at, dimple_at) = if ff.flip { (Vec2::new(-p.x, p.y), Vec2::new(-q.x, q.y)) } else { (p, q) };
        let boxed = |c: Vec2, w: f64| {
            Loop2::polygon(&[Vec2::new(c.x - w, c.y - w), Vec2::new(c.x + w, c.y - w), Vec2::new(c.x + w, c.y + w), Vec2::new(c.x - w, c.y + w)])
        };
        for i in 1..k {
            let b = a0 + f64::from(i) * each;
            // The starting panel's finger before the boundary (its face looks along +u) or after it.
            let before = (i - 1) % 2 == 0;
            let dir = if before { 1.0 } else { -1.0 };
            let face = b - dir * clear / 2.0;
            let centre = face - dir * back;
            let (lo, hi) = (face - dir * over, face + dir * (fit.height + over));
            let (u0, u1) = if lo < hi { (lo, hi) } else { (hi, lo) };
            let nub = kernel::boolean(
                &kernel::sphere(at(centre, nub_at), ball)?,
                &tool_at(&boxed(nub_at, ball + over), u0, u1)?,
                kernel::BoolOp::Intersect,
            )?
            .ok_or_else(|| DocError::Invalid("a press-fit nub on a bend came out empty".into()))?;
            body = kernel::boolean(&body, &nub, kernel::BoolOp::Union)?
                .ok_or_else(|| DocError::Invalid("a press-fit nub on a bend leaves nothing of the print".into()))?;
            // The dimple: the larger ball's cap in the next finger, from the middle of the gap on
            // (nothing of the nub's own finger, nor of what lies beside it in the flat).
            let w = fit.dimple_radius() + over.min(0.1);
            let (lo, hi) = (face + dir * clear / 2.0, face + dir * (fit.height + fit.tolerance.max(0.0) + over));
            let (u0, u1) = if lo < hi { (lo, hi) } else { (hi, lo) };
            let dimple = kernel::boolean(
                &kernel::sphere(at(centre, dimple_at), ball + fit.tolerance)?,
                &tool_at(&boxed(dimple_at, w), u0, u1)?,
                kernel::BoolOp::Intersect,
            )?
            .ok_or_else(|| DocError::Invalid("a press-fit dimple on a bend came out empty".into()))?;
            body = cut(body, dimple)?;
        }
        Ok(body)
    }

    /// What of a joined bend's flat moves with the other panel when folded: each finger's part
    /// past the sharp (above the film, where the other panel is cut back), as tools for the
    /// parent's piece and for the flange's.
    fn fold_moves(&self, start: Vec2, end: Vec2, f: &super::SheetFlange, ff: &FoldFilament, pf: PrintFold) -> Result<Vec<(bool, Body)>> {
        use std::f64::consts::{FRAC_PI_2, PI};
        let Some(lay) = self.fold_layout(start, end, f, ff, pf)? else { return Ok(Vec::new()) };
        let Some(d) = (end - start).normalized() else { return Ok(Vec::new()) };
        // A finger reaches past the sharp only at a fold under 90°: there, between the sharp's
        // plane and the line the clearance and the film short of the other panel's outer skin
        // once folded (half the clearance further, so nothing of that panel is in it).
        let angle = f.angle.abs();
        let cos = angle.cos();
        let m = pf.film + pf.clearance / 2.0;
        let rr = lay.core.r + pf.clearance / 2.0;
        if !(cos > 1e-6 && m / cos < rr) {
            return Ok(Vec::new());
        }
        let pol = |rad: f64, a: f64| Vec2::new(rad * a.cos(), rad * a.sin());
        let z0 = m / cos;
        let phi = angle + (m / rr).asin();
        if !(phi < FRAC_PI_2) {
            return Ok(Vec::new());
        }
        let parent_part = Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::new(0.0, z0), b: pol(rr, phi) },
                Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: rr, start: phi, sweep: FRAC_PI_2 - phi },
                Seg2::Line { a: Vec2::new(0.0, rr), b: Vec2::new(0.0, z0) },
            ],
        };
        let flange_part = Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::new(0.0, z0), b: Vec2::new(0.0, rr) },
                Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: rr, start: FRAC_PI_2, sweep: FRAC_PI_2 - phi },
                Seg2::Line { a: pol(rr, PI - phi), b: Vec2::new(0.0, z0) },
            ],
        };
        let mut out = Vec::with_capacity(lay.k as usize);
        for i in 0..lay.k {
            let parent = (i % 2 == 0) != ff.flip;
            let (s, e) = (lay.a0 + f64::from(i) * lay.each, lay.a0 + f64::from(i + 1) * lay.each);
            let mid = start + d * ((s + e) / 2.0);
            let plane = Plane { origin: Vec3::new(mid.x, mid.y, 0.0), x: Vec3::new(f.n.x, f.n.y, 0.0), y: Vec3::Z };
            let lp = if parent { &parent_part } else { &flange_part };
            let half = (e - s) / 2.0;
            out.push((parent, extrude_flat(&plane, &[Region2 { outer: lp.ccw(), holes: Vec::new() }], -half, half)?));
        }
        Ok(out)
    }

    /// The corners of the folded print where a bend ends against other faces: each corner (in
    /// the flat's frame, folded) and the panels whose outer faces meet there. Only corners on the
    /// outside of the body (every face's outer skin behind each other face's plane there).
    fn print_corners(&self) -> Vec<PrintCorner> {
        let t = self.t;
        let reach = 3.0 * t + self.flanges.iter().map(|f| self.allowance(f)).fold(0.0, f64::max);
        let owners: Vec<Option<usize>> = std::iter::once(None).chain((0..self.flanges.len()).map(Some)).collect();
        // Each panel's fold, inverse, outward normal and a point of its outer face once folded,
        // and its flat regions (the panel and the halves of its bend zones).
        let mut panels = Vec::with_capacity(owners.len());
        for owner in &owners {
            let m = self.panel_fold(*owner);
            let Some(inv) = crate::mat_inverse(&m) else { continue };
            let Some(n) = crate::apply_vector(&m, Vec3::new(0.0, 0.0, -1.0)).normalized() else { continue };
            let regions: Vec<Vec<Vec2>> = self.piece_regions(*owner).iter().map(|r| r.outer.ccw().polyline(1e-3).iter().copied().collect()).collect();
            let Some(p) = regions.first().and_then(|r| r.first()).copied() else { continue };
            let o = crate::apply_point(&m, Vec3::new(p.x, p.y, 0.0));
            panels.push((*owner, inv, n, o, regions));
        }
        let near = |regions: &[Vec<Vec2>], q: Vec2| regions.iter().map(|r| dist_to_polygon(r, q)).fold(f64::INFINITY, f64::min);
        let mut corners: Vec<PrintCorner> = Vec::new();
        for (i, f) in self.flanges.iter().enumerate() {
            let pm = self.panel_fold(f.parent);
            let ba = self.allowance(f);
            let Some(dir) = crate::apply_vector(&pm, Vec3::new(f.d.x, f.d.y, 0.0)).normalized() else { continue };
            let (u0, u1) = self.zone_span(f);
            for (u_end, beyond) in [(u0, false), (u1, true)] {
                let q = f.p + f.n * (ba / 2.0) + f.d * u_end;
                let e = crate::apply_point(&pm, Vec3::new(q.x, q.y, 0.0));
                let sign = if beyond { 1.0 } else { -1.0 };
                for (owner, inv, n, o, regions) in &panels {
                    if *owner == f.parent || *owner == Some(i) {
                        continue;
                    }
                    let dn = dir.dot(*n);
                    if dn.abs() < 0.05 {
                        continue;
                    }
                    let s = (*o - e).dot(*n) / dn;
                    if !(s * sign >= -1e-6 && s.abs() <= reach) {
                        continue;
                    }
                    let v = e + dir * s;
                    let vx = crate::apply_point(inv, v);
                    if near(regions, Vec2::new(vx.x, vx.y)) > reach {
                        continue;
                    }
                    let bend = (i, u_end, beyond);
                    match corners.iter_mut().find(|c| c.v.dist(v) <= 1e-3) {
                        Some(c) => {
                            if !c.bends.contains(&bend) {
                                c.bends.push(bend);
                            }
                        }
                        None => corners.push(PrintCorner { v, faces: Vec::new(), bends: vec![bend] }),
                    }
                }
            }
        }
        // Every panel whose outer face runs through a corner, near it.
        for c in &mut corners {
            for (owner, inv, n, o, regions) in &panels {
                let vx = crate::apply_point(inv, c.v);
                if (c.v - *o).dot(*n).abs() <= 1e-3 && near(regions, Vec2::new(vx.x, vx.y)) <= reach {
                    c.faces.push((*owner, *n));
                }
            }
        }
        // On the outside of the body: a little into each face from the corner lies behind every
        // other face's plane.
        corners.retain(|c| {
            c.faces.len() >= 3
                && c.faces.iter().all(|(owner, _)| {
                    let Some((_, inv, _, _, regions)) = panels.iter().find(|p| p.0 == *owner) else { return false };
                    let Some(m) = crate::mat_inverse(inv) else { return false };
                    let vx = crate::apply_point(inv, c.v);
                    let vx = Vec2::new(vx.x, vx.y);
                    let pts: Vec<Vec2> = regions.iter().flatten().copied().collect();
                    let centre = pts.iter().fold(Vec2::new(0.0, 0.0), |s, p| s + *p) * (1.0 / pts.len().max(1) as f64);
                    let Some(towards) = (centre - vx).normalized() else { return false };
                    let q = vx + towards * (2.0 * t).min((centre - vx).len());
                    let q = crate::apply_point(&m, Vec3::new(q.x, q.y, 0.0));
                    c.faces.iter().filter(|(o, _)| o != owner).all(|(_, n)| (q - c.v).dot(*n) <= 1e-3)
                })
        });
        corners
    }

    /// The filament channels that run into corner `c`, folded: a joined bend's that ends there
    /// (square to its parent's wall) and an open edge's that ends there (square to its panel's),
    /// each run on `reach` past both ends of its edge.
    fn corner_channels(&self, c: &PrintCorner, pf: PrintFold, reach: f64) -> Result<Vec<Body>> {
        let t = self.t;
        let square = |x: f64, z: f64, h: f64| {
            Loop2::polygon(&[Vec2::new(x - h, z - h), Vec2::new(x + h, z - h), Vec2::new(x + h, z + h), Vec2::new(x - h, z + h)])
        };
        let mut out = Vec::new();
        for (i, u_end, _) in &c.bends {
            let (Some(f), Some(ff)) = (self.flanges.get(*i), self.fold_filament.iter().find(|ff| ff.flange == *i)) else { continue };
            if ff.press.is_some() {
                continue;
            }
            let Ok(core) = ff.core(t, f.angle, pf) else { continue };
            let half = f.angle.abs() / 2.0;
            let ba = self.allowance(f);
            // Across the bend from its sharp toward the flange, and up: the parent's square.
            let mid = f.p + f.d * *u_end + f.n * (ba / 2.0);
            let plane = Plane { origin: Vec3::new(mid.x, mid.y, 0.0), x: Vec3::new(f.n.x, f.n.y, 0.0), y: Vec3::Z };
            let lp = square(-core.d * half.sin(), core.d * half.cos(), ff.side / 2.0);
            let tube = extrude_flat(&plane, &[Region2 { outer: lp.ccw(), holes: Vec::new() }], -reach, reach)?;
            out.push(kernel::transform_matrix(&tube, self.panel_fold(f.parent))?);
        }
        for fc in &self.filament {
            if fc.press.is_some() || !c.faces.iter().any(|(o, _)| *o == fc.panel) {
                continue;
            }
            let m = self.panel_fold(fc.panel);
            let ends = [fc.a, fc.b].map(|p| crate::apply_point(&m, Vec3::new(p.x, p.y, 0.0)));
            let Some(end) = ends.iter().copied().find(|p| p.dist(c.v) <= reach) else { continue };
            let Some(inv) = crate::mat_inverse(&m) else { continue };
            let e = crate::apply_point(&inv, end);
            let plane = Plane { origin: Vec3::new(e.x, e.y, 0.0), x: Vec3::new(fc.n.x, fc.n.y, 0.0), y: Vec3::Z };
            let lp = square(fc.offset, t / 2.0, fc.side / 2.0);
            let tube = extrude_flat(&plane, &[Region2 { outer: lp.ccw(), holes: Vec::new() }], -reach, reach)?;
            out.push(kernel::transform_matrix(&tube, m)?);
        }
        Ok(out)
    }

    /// The flat to folded transform of a panel (none: the base).
    fn panel_fold(&self, owner: Option<usize>) -> crate::Mat {
        owner.map_or(crate::IDENTITY, |i| self.fold(i))
    }

    /// What closes each corner where a bend ends ([`print_corners`](Self::print_corners)): the
    /// bend zones stop short of the other faces there, so part of the corner the walls share is
    /// nobody's. Within the walls (inside every outer face, not inside all the inner ones), where
    /// the zones would have run on, and outside every panel's own flat (its panel and half
    /// zones), each face of the corner takes the part on its side of the planes halving the angle
    /// to the others, half the clearance short of them, as a mitred box's corner does. Each part
    /// in its panel's flat, with that panel. Under them each bend's film runs on to the corner,
    /// so the parts either side of a bend hang together on it as its zones do.
    fn corner_fills(&self, pf: PrintFold) -> Result<Vec<(Option<usize>, Body)>> {
        let t = self.t;
        // A solid that only grazes another face (no face of one lying on a face of the other)
        // keeps every boolean below clear of tangled slivers: the pieces overlap by this much.
        let lap = (t * 0.02).max(0.01);
        let mut out = Vec::new();
        for c in self.print_corners() {
            let reach = 3.0 * t + c.bends.iter().filter_map(|(i, ..)| self.flanges.get(*i)).map(|f| self.allowance(f)).fold(0.0, f64::max);
            let step = |what: &str, r: std::result::Result<Option<Body>, kernel::KernelError>| -> Result<Option<Body>> {
                r.map_err(|e| {
                    DocError::Invalid(format!("closing the corner of the print at ({:.3}, {:.3}, {:.3}) ({what}): {e}", c.v.x, c.v.y, c.v.z))
                })
            };
            // The half-space (p - v)·m <= k near the corner, its far faces well away.
            let half = |m: Vec3, k: f64, far: f64| -> Result<Body> {
                let Some(m) = m.normalized() else { return Err(DocError::Invalid("a corner of the print has no plane".into())) };
                let seed = if m.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
                let Some(x) = m.cross(seed).normalized() else { return Err(DocError::Invalid("a corner of the print has no plane".into())) };
                let plane = Plane { origin: c.v + m * k, x, y: m.cross(x) };
                let w = far * reach;
                let square = Loop2::polygon(&[Vec2::new(-w, -w), Vec2::new(w, -w), Vec2::new(w, w), Vec2::new(-w, w)]);
                Ok(extrude_flat(&plane, &[Region2 { outer: square.ccw(), holes: Vec::new() }], -w, 0.0)?)
            };
            // Within the walls near the corner: inside every outer face, not inside every inner one.
            let mut outer: Option<Body> = None;
            let mut inner: Option<Body> = None;
            for (k, (_, n)) in c.faces.iter().enumerate() {
                let far = 4.0 + 0.37 * k as f64;
                let (o, i) = (half(*n, 0.0, far)?, half(*n, -t, far + 0.11)?);
                outer = match outer {
                    None => Some(o),
                    Some(b) => step("the outer faces", kernel::boolean(&b, &o, kernel::BoolOp::Intersect))?,
                };
                inner = match inner {
                    None => Some(i),
                    Some(b) => step("the inner faces", kernel::boolean(&b, &i, kernel::BoolOp::Intersect))?,
                };
                if outer.is_none() {
                    break;
                }
            }
            let Some(mut walls) = outer else { continue };
            if let Some(inner) = inner {
                let Some(w) = step("the walls", kernel::boolean(&walls, &inner, kernel::BoolOp::Cut))? else { continue };
                walls = w;
            }
            // Where the bend zones would have run on to the corner (from a little inside their
            // ends, a little past both their edges).
            let mut missing: Option<Body> = None;
            for (k, (i, u_end, beyond)) in c.bends.iter().enumerate() {
                let Some(f) = self.flanges.get(*i) else { continue };
                let ba = self.allowance(f);
                let (s, e) = if *beyond { (*u_end - lap, *u_end + reach) } else { (*u_end - reach, *u_end + lap) };
                for (j, (across, owner)) in [((-lap, ba / 2.0), f.parent), ((ba / 2.0, ba + lap), Some(*i))].into_iter().enumerate() {
                    let at = |u: f64, w: f64| f.p + f.d * u + f.n * w;
                    let r = super::rect(at(s, across.0), at(e, across.0), at(e, across.1), at(s, across.1));
                    // Through the wall (and a little past both skins, by a little more for each
                    // zone, so no two of them have a face in one plane).
                    let deep = lap * (1.0 + 0.1 * (2 * k + j) as f64);
                    let prism = kernel::transform_matrix(&extrude_flat(&Plane::XY, &[r], -deep, t + deep)?, self.panel_fold(owner))?;
                    let Some(part) = step("a bend zone run on", kernel::boolean(&walls, &prism, kernel::BoolOp::Intersect))? else { continue };
                    missing = match missing {
                        None => Some(part),
                        Some(m) => step("the bend zones run on", kernel::boolean(&m, &part, kernel::BoolOp::Union))?,
                    };
                }
            }
            let Some(mut missing) = missing else { continue };
            // Outside every panel's own flat (a little past its inner skin too).
            'faces: for (owner, _) in &c.faces {
                for r in self.piece_regions(*owner) {
                    let slab = kernel::transform_matrix(&extrude_flat(&Plane::XY, &[r], -reach, t + lap)?, self.panel_fold(*owner))?;
                    match step("a panel's own flat", kernel::boolean(&missing, &slab, kernel::BoolOp::Cut))? {
                        Some(m) => missing = m,
                        None => break 'faces,
                    }
                }
            }
            // The channels that run through the corner stay open: each channel on with its square,
            // past the ends of its edge.
            for tube in self.corner_channels(&c, pf, reach)? {
                match step("a channel", kernel::boolean(&missing, &tube, kernel::BoolOp::Cut))? {
                    Some(m) => missing = m,
                    None => break,
                }
            }
            // Each bend's film runs on to the corner, each half under its side's part: on it the
            // part of a panel that folds hangs on the part of the one it folds from, as the
            // bend's zones hang together, where by its end face alone it would break off. The
            // half of `owner`'s side of the bend between it and `other`, through the film and
            // a little past the outer skin, up to the bend's middle.
            let film = |owner: Option<usize>, other: Option<usize>| -> Result<Option<Body>> {
                for (k, (i, u_end, beyond)) in c.bends.iter().enumerate() {
                    let Some(f) = self.flanges.get(*i) else { continue };
                    let ba = self.allowance(f);
                    let across = if owner == f.parent && other == Some(*i) {
                        (-lap / 2.0, ba / 2.0)
                    } else if owner == Some(*i) && other == f.parent {
                        (ba / 2.0, ba + lap / 2.0)
                    } else {
                        continue;
                    };
                    let (s, e) = if *beyond { (*u_end - lap / 2.0, *u_end + 0.9 * reach) } else { (*u_end - 0.9 * reach, *u_end + lap / 2.0) };
                    let at = |u: f64, w: f64| f.p + f.d * u + f.n * w;
                    let r = super::rect(at(s, across.0), at(e, across.0), at(e, across.1), at(s, across.1));
                    let deep = lap * (1.5 + 0.1 * k as f64);
                    return Ok(Some(kernel::transform_matrix(&extrude_flat(&Plane::XY, &[r], -deep, pf.film)?, self.panel_fold(owner))?));
                }
                Ok(None)
            };
            // Each face's part, half the clearance short of the planes halving its angles.
            for (owner, n) in &c.faces {
                let mut part = Some(missing.clone());
                for (k, (other, m)) in c.faces.iter().enumerate() {
                    if other == owner {
                        continue;
                    }
                    let Some(u) = (*n - *m).normalized() else { continue };
                    let Some(p) = part else { break };
                    let mut keep = half(-u, -pf.clearance / 2.0, 5.0 + 0.13 * k as f64)?;
                    if let Some(strip) = film(*owner, *other)? {
                        keep = step("a bend's film", kernel::boolean(&keep, &strip, kernel::BoolOp::Union))?.unwrap_or(keep);
                    }
                    part = step("a face's part", kernel::boolean(&p, &keep, kernel::BoolOp::Intersect))?;
                }
                let Some(part) = part else { continue };
                if part.volume_at(1e-4).is_none_or(|v| v < 1e-2) {
                    continue;
                }
                let Some(inv) = crate::mat_inverse(&self.panel_fold(*owner)) else { continue };
                let flat = kernel::transform_matrix(&part, inv)?;
                // Only a part that hangs on its panel's flat, and no sliver: anything else would
                // print as a loose crumb (a corner where the panel is cut back from the others).
                let Ok(b) = flat.exact_bounds() else { continue };
                let size = b.max - b.min;
                if size.x.min(size.y).min(size.z) < 0.1 {
                    continue;
                }
                let regions: Vec<Vec<Vec2>> =
                    self.piece_regions(*owner).iter().map(|r| r.outer.ccw().polyline(1e-3).iter().copied().collect()).collect();
                let (lo, hi) = (Vec2::new(b.min.x, b.min.y), Vec2::new(b.max.x, b.max.y));
                if !regions.iter().any(|r| touches_box(r, lo, hi, 2.0 * lap)) {
                    continue;
                }
                out.push((*owner, flat));
            }
        }
        Ok(out)
    }

    /// The flat body of a foldable print, placed in the sheet's frame: the flat pattern, its
    /// pockets and bores, and a V-groove at every bend ([`check_print_fold`] first).
    pub fn print_fold_solid(&self, pf: PrintFold) -> Result<Body> {
        self.place_local_solid(self.print_fold_local(pf)?)
    }

    /// The flat body of a foldable print in the flat's own coordinates (the outer skin on z = 0).
    pub fn print_fold_local(&self, pf: PrintFold) -> Result<Body> {
        Ok(self.print_fold_local_fills(pf)?.0)
    }

    /// [`print_fold_local`](Self::print_fold_local), and the corner fills in it, each with the
    /// panel it folds with ([`corner_fills`](Self::corner_fills)).
    fn print_fold_local_fills(&self, pf: PrintFold) -> Result<(Body, Vec<(Option<usize>, Body)>)> {
        check_print_fold(self, pf)?;
        let mut regions = self.flat_regions()?;
        if !self.pockets.is_empty() {
            regions = regions.iter().map(super::fitted_region).collect::<Result<_>>()?;
        }
        let mut local = self.clone();
        local.frame = Plane::XY;
        local.flat = true;
        let t = self.t;
        let mut body = extrude_flat(&Plane::XY, &regions, 0.0, t)?;
        let mut work = FlatWork::new();
        body = local.machine_pockets(body, &mut work)?;
        body = local.machine_bores(body, &mut work)?;
        // The groove's tool runs a little past the top and past the bend's ends, so no face of
        // it lies on a face of the sheet.
        let over = (t * 0.05).max(1e-3);
        // Every channel is cut last, through the corner fills too.
        let mut channels: Vec<Body> = Vec::new();
        for b in self.bends() {
            let Some(f) = b.index.checked_sub(1).and_then(|i| self.flanges.get(i)) else { continue };
            if let Some(ff) = self.fold_filament.iter().find(|ff| ff.flange + 1 == b.index) {
                body = self.fold_fingers(body, b.start, b.end, f, ff, pf, over, b.index, &mut channels)?;
                continue;
            }
            let along = b.end - b.start;
            let len = along.len();
            let Some(d) = along.normalized() else { continue };
            body = self.v_groove(body, b.start, d, f, pf, over, -over, len + over, b.index)?;
        }
        // Mitred ends: the wedge between the square end and the mitre, from nothing at the outer
        // skin to `extra` at the inner one (and on past both, so no face of it lies on the sheet's).
        for v in &self.bevels {
            let along = v.b - v.a;
            let len = along.len();
            if !(v.extra > 0.0 && len > 0.0) {
                continue;
            }
            let top = t + over;
            let reach_in = v.at + v.extra * top / t;
            let profile = Loop2::polygon(&[Vec2::new(v.at - over, 0.0), Vec2::new(v.at, 0.0), Vec2::new(reach_in, top), Vec2::new(v.at - over, top)]);
            let mid = v.a + along * 0.5;
            let plane = Plane { origin: Vec3::new(mid.x, mid.y, 0.0), x: Vec3::new(v.n.x, v.n.y, 0.0), y: Vec3::Z };
            let reach = len / 2.0 + over;
            let tool = extrude_flat(&plane, &[Region2 { outer: profile.ccw(), holes: Vec::new() }], -reach, reach)?;
            body = kernel::boolean(&body, &tool, kernel::BoolOp::Cut)?
                .ok_or_else(|| DocError::Invalid("a mitred end leaves nothing of the print".into()))?;
        }
        // Filament joints: the other panel's fingers notched out of this one, then the channel
        // through the fingers along the edge.
        let cut = |body: Body, tool: Body| -> Result<Body> {
            kernel::boolean(&body, &tool, kernel::BoolOp::Cut)?
                .ok_or_else(|| DocError::Invalid("a filament joint leaves nothing of the print".into()))
        };
        // Where a channel runs out through a face folded over its end.
        for hole in &self.channel_holes {
            if hole.len() < 3 {
                continue;
            }
            channels.push(extrude_flat(&Plane::XY, &[Region2 { outer: Loop2::polygon(hole).ccw(), holes: Vec::new() }], -over, t + over)?);
        }
        for fc in &self.filament {
            let along = fc.b - fc.a;
            let len = along.len();
            let Some(d) = along.normalized() else { continue };
            if let Some(fit) = fc.press {
                fit.check(pf.clearance, t / 2.0 - pf.film, t).map_err(|why| DocError::Invalid(format!("a press fit: {why}")))?;
            } else if !(fc.side > 0.0 && fc.side + 2.0 * pf.film <= t) {
                return Err(DocError::Invalid(format!(
                    "a filament channel {:.2} mm wide needs walls at least {:.2} mm thick (the channel and the film's thickness either side)",
                    fc.side,
                    fc.side + 2.0 * pf.film
                )));
            }
            for (u0, u1) in &fc.notches {
                let p = |u: f64, w: f64| fc.a + d * u + fc.n * w;
                let rect = Loop2::polygon(&[p(*u0, -over), p(*u1, -over), p(*u1, fc.depth), p(*u0, fc.depth)]);
                let tool = extrude_flat(&Plane::XY, &[Region2 { outer: rect.ccw(), holes: Vec::new() }], -over, t + over)?;
                body = cut(body, tool)?;
            }
            // At a fold under 90° the corner both walls share reaches past this panel's edge, up
            // to the other's outer face: each of this panel's fingers takes it, so the corner is
            // closed. In the flat it stands past the edge, leaning out by the fold (its top dips
            // a hair into the panel, so no face of it lies on the sheet's).
            let reach = fc.reach(t);
            if reach > 1e-9 {
                let eps = (over * 0.1).min(0.005);
                let wedge = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(reach, t), Vec2::new(-eps, t - eps)]);
                // In the flat the panel across the joint lies beside this one, turned away by the
                // corner of the face both hang on: past the line halfway between their two edges
                // a finger leaning out would stand in that panel. It ends short of that line.
                let mut sides: Vec<Body> = Vec::new();
                for other in self.filament.iter().filter(|o| o.panel != fc.panel) {
                    let (r, q) = (fc.b - fc.a, other.b - other.a);
                    let den = r.cross(q);
                    if den.abs() < 1e-9 {
                        continue;
                    }
                    let c = fc.a + r * ((other.a - fc.a).cross(q) / den);
                    let near = |a: Vec2, b: Vec2| (c - a).len().min((c - b).len()) <= 4.0 * t + reach;
                    if !(near(fc.a, fc.b) && near(other.a, other.b)) {
                        continue;
                    }
                    let (Some(own), Some(theirs)) = (((fc.a + fc.b) * 0.5 - c).normalized(), ((other.a + other.b) * 0.5 - c).normalized()) else {
                        continue;
                    };
                    if own.dot(theirs).abs() > 0.996 {
                        continue;
                    }
                    let Some(m) = (own + theirs).normalized() else { continue };
                    let side = if Vec2::new(-m.y, m.x).dot(own) >= 0.0 { Vec2::new(-m.y, m.x) } else { Vec2::new(m.y, -m.x) };
                    let (far, gap) = (len + q.len() + 10.0 * t + reach, FINGER_FLAT_GAP / 2.0);
                    let p = |along: f64, across: f64| c + m * along + side * across;
                    let half = Loop2::polygon(&[p(-far, gap), p(far, gap), p(far, gap + far), p(-far, gap + far)]);
                    sides.push(extrude_flat(&Plane::XY, &[Region2 { outer: half.ccw(), holes: Vec::new() }], -2.0 * over, t + 2.0 * over)?);
                }
                'fingers: for (s, e) in fc.own_fingers() {
                    let mid = fc.a + d * ((s + e) / 2.0);
                    let plane = Plane { origin: Vec3::new(mid.x, mid.y, 0.0), x: Vec3::new(-fc.n.x, -fc.n.y, 0.0), y: Vec3::Z };
                    let mut tool = extrude_flat(&plane, &[Region2 { outer: wedge.ccw(), holes: Vec::new() }], -(e - s) / 2.0, (e - s) / 2.0)?;
                    if !sides.is_empty() {
                        let whole = kernel::measure(&tool)?.volume;
                        for side in &sides {
                            match kernel::boolean(&tool, side, kernel::BoolOp::Intersect)? {
                                Some(left) => tool = left,
                                None => continue 'fingers,
                            }
                        }
                        // What is left of a finger cut back to a fin would break off in folding.
                        if kernel::measure(&tool)?.volume < 0.6 * whole {
                            continue 'fingers;
                        }
                    }
                    body = kernel::boolean(&body, &tool, kernel::BoolOp::Union)?
                        .ok_or_else(|| DocError::Invalid("a joint on an open edge leaves nothing of the print".into()))?;
                }
            }
            if let Some(fit) = fc.press {
                body = self.press_nubs(body, fc, &fit, d, len, pf.clearance, over)?;
                continue;
            }
            let h = fc.side / 2.0;
            let (o, z) = (fc.offset, t / 2.0);
            let square = Loop2::polygon(&[Vec2::new(o - h, z - h), Vec2::new(o + h, z - h), Vec2::new(o + h, z + h), Vec2::new(o - h, z + h)]);
            let plane = Plane { origin: Vec3::new(fc.a.x, fc.a.y, 0.0), x: Vec3::new(fc.n.x, fc.n.y, 0.0), y: Vec3::Z };
            // The plane's normal (n × z) runs along the edge one way or the other.
            let (lo, hi) = if fc.n.y * d.x - fc.n.x * d.y >= 0.0 { (-over, len + over) } else { (-len - over, over) };
            channels.push(extrude_flat(&plane, &[Region2 { outer: square.ccw(), holes: Vec::new() }], lo, hi)?);
        }
        // The corners the bend zones leave open, closed; then every channel.
        // A fill that is a thin fin (a corner where the walls meet at a shallow angle) hangs on
        // the film alone and breaks off in folding: the corner stays open there.
        let mut fills = self.corner_fills(pf)?;
        let mut sturdy = Vec::with_capacity(fills.len());
        for (owner, fill) in fills.drain(..) {
            if kernel::measure(&fill)?.volume >= FIN * t * t * t {
                sturdy.push((owner, fill));
            }
        }
        let fills = sturdy;
        for (_, fill) in &fills {
            body = kernel::boolean(&body, fill, kernel::BoolOp::Union)?
                .ok_or_else(|| DocError::Invalid("a corner of the print leaves nothing of it".into()))?;
        }
        for tool in channels {
            body = cut(body, tool)?;
        }
        // Crumbs the cuts left loose (a sliver between a channel and a groove) are no part of the
        // print: they would print as loose bits beside it.
        let lumps = body.lumps()?;
        if lumps.len() > 1 {
            let mut kept = Vec::with_capacity(lumps.len());
            for lump in lumps {
                if kernel::measure(&lump)?.volume >= CRUMB {
                    kept.push(lump);
                }
            }
            if kept.len() == 1 {
                body = kept.remove(0);
            } else if let Some(joined) = kernel::join_apart(&kept)? {
                body = joined;
            }
        }
        Ok((body, fills))
    }

    /// A press fit's nubs or dimples on one panel's fingers along an open edge: at each end face
    /// of a finger (where a notch for the other panel's finger begins or ends, not at the edge's
    /// ends), in the middle of the folded corner. A nub is the cap of a ball whose centre lies
    /// behind the face; a dimple is that ball (the other panel's nub), `tolerance` larger.
    #[allow(clippy::too_many_arguments)]
    fn press_nubs(&self, mut body: Body, fc: &FilamentCut, fit: &PressFit, d: Vec2, len: f64, clearance: f64, over: f64) -> Result<Body> {
        let t = self.t;
        let (ball, back) = (fit.ball(), fit.ball() - fit.height);
        let at = |u: f64| {
            let p = fc.a + d * u + fc.n * fc.offset;
            Vec3::new(p.x, p.y, t / 2.0)
        };
        for (lo, hi) in &fc.notches {
            // This panel's finger ends at `lo` looking along +u into the notch, and starts again at `hi`.
            for (face, dir) in [(*lo, 1.0), (*hi, -1.0)] {
                if face <= 0.0 || face >= len {
                    continue;
                }
                if fc.nubs {
                    let centre = face - dir * back;
                    let (u0, u1) = (face - dir * over, face + dir * (fit.height + over));
                    let (u0, u1) = if u0 < u1 { (u0, u1) } else { (u1, u0) };
                    let w = ball + over;
                    let p = |u: f64, x: f64| fc.a + d * u + fc.n * x;
                    let o = fc.offset;
                    let rect = Loop2::polygon(&[p(u0, o - w), p(u1, o - w), p(u1, o + w), p(u0, o + w)]);
                    let cap = extrude_flat(&Plane::XY, &[Region2 { outer: rect.ccw(), holes: Vec::new() }], t / 2.0 - w, t / 2.0 + w)?;
                    let nub = kernel::boolean(&kernel::sphere(at(centre), ball)?, &cap, kernel::BoolOp::Intersect)?
                        .ok_or_else(|| DocError::Invalid("a press-fit nub came out empty".into()))?;
                    body = kernel::boolean(&body, &nub, kernel::BoolOp::Union)?
                        .ok_or_else(|| DocError::Invalid("a press-fit nub leaves nothing of the print".into()))?;
                } else {
                    // The other panel's finger stands `clearance` away, its nub's ball centre behind
                    // it; the dimple is the larger ball's cap beyond that face, nothing behind it.
                    let nub_face = face + dir * clearance;
                    let centre = nub_face + dir * back;
                    let eps = over.min(0.1);
                    let (u0, u1) = (nub_face + dir * eps, nub_face - dir * (fit.height + fit.tolerance.max(0.0) + over));
                    let (u0, u1) = if u0 < u1 { (u0, u1) } else { (u1, u0) };
                    let w = fit.dimple_radius() + eps;
                    let p = |u: f64, x: f64| fc.a + d * u + fc.n * x;
                    let o = fc.offset;
                    let rect = Loop2::polygon(&[p(u0, o - w), p(u1, o - w), p(u1, o + w), p(u0, o + w)]);
                    let open = extrude_flat(&Plane::XY, &[Region2 { outer: rect.ccw(), holes: Vec::new() }], t / 2.0 - w, t / 2.0 + w)?;
                    let dimple = kernel::boolean(&kernel::sphere(at(centre), ball + fit.tolerance)?, &open, kernel::BoolOp::Intersect)?
                        .ok_or_else(|| DocError::Invalid("a press-fit dimple came out empty".into()))?;
                    body = kernel::boolean(&body, &dimple, kernel::BoolOp::Cut)?
                        .ok_or_else(|| DocError::Invalid("a press-fit dimple leaves nothing of the print".into()))?;
                }
            }
        }
        Ok(body)
    }

    /// The box round the flat in its own coordinates: (least x, least y), (most x, most y).
    pub fn flat_box(&self) -> Result<(Vec2, Vec2)> {
        let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
        let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for r in self.flat_regions()? {
            for p in r.outer.polyline(1e-3).iter() {
                lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
            }
        }
        if !(lo.x <= hi.x && lo.y <= hi.y) {
            return Err(DocError::Invalid("empty flat pattern".into()));
        }
        Ok((lo, hi))
    }

    /// Two pieces of the flat (panels, and bend zones with their flanges) that overlap, by owner:
    /// None the base, Some(i) flange i. Pieces that only touch along an edge or at a corner don't.
    pub fn print_overlap(&self) -> Option<(Option<usize>, Option<usize>)> {
        let ring = |l: &Loop2| -> Vec<Vec2> { l.ccw().polyline(1e-3).iter().copied().collect() };
        let piece = |o: Option<usize>, r: Region2| (o, ring(&r.outer), r.holes.iter().map(ring).collect::<Vec<_>>());
        let mut pieces: Vec<(Option<usize>, Vec<Vec2>, Vec<Vec<Vec2>>)> = self.panels().into_iter().map(|(o, r)| piece(o, r)).collect();
        pieces.extend(self.flanges.iter().enumerate().map(|(i, f)| piece(Some(i), self.zone(f))));
        let eps = (self.t * 1e-3).max(1e-6);
        // A piece that lies in a hole of another (a wall folding into its panel's hole) is clear of it.
        let in_hole = |p: &[Vec2], holes: &[Vec<Vec2>]| holes.iter().any(|h| p.iter().all(|q| dist_to_polygon(h, *q) <= eps));
        for (k, (a, pa, ha)) in pieces.iter().enumerate() {
            for (b, pb, hb) in pieces.iter().skip(k + 1) {
                if a != b && overlaps(pa, pb, eps) && !in_hole(pb, ha) && !in_hole(pa, hb) {
                    return Some((*a, *b));
                }
            }
        }
        None
    }
}

/// Whether two simple polygons share some area: their edges cross, or a corner or an edge's
/// middle of one lies inside the other, more than `eps` from its outline.
fn overlaps(a: &[Vec2], b: &[Vec2], eps: f64) -> bool {
    if a.len() < 3 || b.len() < 3 {
        return false;
    }
    let deep_in = |p: Vec2, poly: &[Vec2]| inside(p, poly) && edges(poly).all(|(s, e)| seg_dist(p, s, e) > eps);
    let probes = |poly: &[Vec2]| -> Vec<Vec2> { edges(poly).flat_map(|(s, e)| [s, (s + e) * 0.5]).collect() };
    super::polys_cross(a, b) || probes(a).into_iter().any(|p| deep_in(p, b)) || probes(b).into_iter().any(|p| deep_in(p, a))
}

fn edges(poly: &[Vec2]) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    poly.iter().enumerate().filter_map(|(i, s)| Some((*s, *poly.get((i + 1) % poly.len())?)))
}

/// The distance from `p` to the segment from `s` to `e`.
fn seg_dist(p: Vec2, s: Vec2, e: Vec2) -> f64 {
    let d = e - s;
    let l2 = d.dot(d);
    let u = if l2 > 0.0 { ((p - s).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(s + d * u)
}

/// Even-odd point in polygon.
fn inside(p: Vec2, poly: &[Vec2]) -> bool {
    edges(poly).fold(false, |c, (s, e)| if (s.y > p.y) != (e.y > p.y) && p.x < s.x + (p.y - s.y) / (e.y - s.y) * (e.x - s.x) { !c } else { c })
}
