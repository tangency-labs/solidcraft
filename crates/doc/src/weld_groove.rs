//! Groove welds (#18500): the joint of a groove (butt) weld, read from the two parts the user
//! picked: each part's groove faces as one chain from the toe ([`chain`]), the joint's direction
//! and frame ([`joint_frame`]), and the weld that fills the groove ([`groove_weld`]), or one
//! stretch of it that keeps one section ([`groove_weld_over`]).
//!
//! SolidCraft's own: solidcore and solidcore-ops hold no weld (bherbruck on #18409). Every read
//! here is an ops call; this module adds the weld's rules and refusals. Nothing writes to a part.
//!
//! | Rust              | What it is                                                         |
//! |-------------------|--------------------------------------------------------------------|
//! | [`GroovePick`]    | One part's side of the joint: its groove faces and its outer face   |
//! | [`JointPart`]     | Which of the two parts something is about                           |
//! | [`Chain`]         | A part's groove faces in order from the toe, and its toe edges      |
//! | [`chain`]         | The chain of one pick                                               |
//! | [`joint_frame`]   | The joint's frame: a plane whose normal is the joint's direction    |
//! | [`groove_weld`]   | The weld: the groove's walls, its fill and its body                  |
//! | [`groove_weld_over`] | The weld over one stretch of the joint                           |
//! | [`Stretch`]       | A stretch of the joint that keeps one section                       |
//! | [`GrooveWeld`]    | What [`groove_weld`] gives                                          |
//! | [`WeldNote`]      | What the weld tool tells the user about a weld that built           |
//! | [`GrooveRefusal`] | Why a joint is refused                                              |
//!
//! # Not covered
//!
//! T joints, whose toe on the through part is no edge, are refused ([`GrooveRefusal::TJoint`]);
//! their toe and the seam mark come in a later PR (brett19 on #18500). A joint whose section
//! changes along it, or whose parts do not run alongside each other all along it, is refused
//! with the stretches that keep one section ([`GrooveRefusal::SectionChanges`], brett19 on
//! #18500, Q56); each one builds on its own ([`groove_weld_over`]). Building them all at once
//! comes later. [`joint_frame`] reads one chain over the whole joint, so a groove face split
//! along the joint into faces side by side is refused there ([`GrooveRefusal::ArrowFaceNotAtToe`]);
//! [`groove_weld`] reads the chain piece by piece (brett19's point 7 on #22033). Faces that lie
//! on one surface are read as separate faces.

use solidcore_record::{Plane, Seg2, Vec2, Vec3};
use solidcraft_kernel::services::measure::{Straight, straight_direction};
use solidcraft_kernel::services::prism::{
    ChannelFill, FillFailure, FillNote, FillOptions, FillSide, PlanarPiece, extrude_planar_regions, fill_channel, planar_edges,
};
use solidcraft_kernel::services::{ExactBody as Body, Order, affine, sweep};

/// Which of the two parts of a joint something is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JointPart {
    /// The left part (the first one passed).
    Left,
    /// The right part.
    Right,
}

/// Why a groove joint is refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum GrooveRefusal {
    /// Input the joint cannot take: no groove face picked, or a face the part does not have.
    #[error("{0}")]
    Invalid(&'static str),
    /// The groove faces picked on a part do not form one chain of faces that share edges, from
    /// the toe to the root: record face `face` is off the chain (not joined to it, or a third
    /// neighbour of a face on it).
    #[error("the {part:?} part's groove faces are not one chain: face {face} is off it")]
    PicksNotOneChain { part: JointPart, face: usize },
    /// A part's outer face `face` (on the arrow side) does not meet its groove faces at one end
    /// of their chain: there is no toe.
    #[error("the {part:?} part's outer face {face} does not meet its groove faces at a toe")]
    ArrowFaceNotAtToe { part: JointPart, face: usize },
    /// A part's outer face is one of its groove faces: a T joint, whose toe on the through part
    /// is no edge. Not built yet.
    #[error("the {part:?} part's outer face is one of its groove faces (a T joint)")]
    TJoint { part: JointPart },
    /// A part's outer face `face` is not a plane.
    #[error("the {part:?} part's outer face {face} is not a plane")]
    OuterFaceNotPlane { part: JointPart, face: usize },
    /// Face `face` of a part (a groove face or its outer face) strays across the joint's direction
    /// by more than the zone tolerance ([`Body::face_straying`]): it does not run straight along
    /// the joint.
    #[error("face {face} of the {part:?} part is not straight along the joint")]
    GrooveFaceNotStraight { part: JointPart, face: usize },
    /// Face `face` of a part (a groove face or its outer face) is a surface whose straightness
    /// along the joint ops cannot prove (no bound from [`Body::face_straying`]): it may be
    /// straight, but it is not taken unproved.
    #[error("face {face} of the {part:?} part cannot be proved straight along the joint")]
    GrooveFaceNotProved { part: JointPart, face: usize },
    /// The faces give no direction for the joint: no two of their planes are proved apart and none
    /// is a cylinder (a groove of other curved faces between plates whose outer faces are
    /// parallel).
    #[error("the faces give no direction for the joint")]
    DirectionUndetermined,
    /// The groove does not keep one section all along the joint: its section changes at a face
    /// boundary or inside a face, or one part has no groove face over part of it. `stretches`
    /// are the stretches that do keep one section, in order along the joint (none, if no stretch
    /// does); [`groove_weld_over`] builds one of them.
    #[error("{}", stretches_text(stretches))]
    SectionChanges { stretches: Vec<Stretch> },
    /// The groove cannot be filled as asked ([`FillFailure`]); `fit` is the nearest size that
    /// builds, where there is one.
    #[error("{}", fill_text(why, *fit))]
    Fill { why: FillFailure, fit: Option<f64> },
    /// An ops call failed.
    #[error(transparent)]
    Ops(#[from] solidcraft_kernel::services::Error),
}

type Result<T> = std::result::Result<T, GrooveRefusal>;

/// One part's side of a groove joint.
#[derive(Debug, Clone, Copy)]
pub struct GroovePick<'a> {
    /// The part (only read).
    pub part: &'a Body,
    /// Its record order: the faces below are record indices in it.
    pub order: &'a Order,
    /// Its groove faces (bevel, root face, J or U radius, square wall), in any order.
    pub faces: &'a [usize],
    /// Its plate face on the arrow side: the side the weld is made from.
    pub outer: usize,
}

/// A part's groove faces in order from the toe. Only [`chain`] makes one, so [`joint_frame`] can
/// trust it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    /// The groove faces (record indices), the one beside the outer face first, each next one
    /// sharing an edge with the one before.
    faces: Vec<usize>,
    /// The toe: the edges (record indices) the first face shares with the outer face.
    toe: Vec<usize>,
}

impl Chain {
    /// The groove faces (record indices), the one beside the outer face first, each next one
    /// sharing an edge with the one before.
    #[must_use]
    pub fn faces(&self) -> &[usize] {
        &self.faces
    }

    /// The toe: the edges (record indices) the first face shares with the outer face.
    #[must_use]
    pub fn toe(&self) -> &[usize] {
        &self.toe
    }
}

/// The picked groove faces of `pick` as one chain from the toe, read from the part's topology
/// only: which faces share an edge ([`Body::rim_edges`] of each face). The order of the picks
/// changes nothing. `part` names the part in a refusal.
///
/// # Errors
/// [`GrooveRefusal::Invalid`] for no groove face, or a face past the part's faces;
/// [`GrooveRefusal::TJoint`] when the outer face is among the groove faces;
/// [`GrooveRefusal::PicksNotOneChain`] when a face has three picked neighbours or is not joined
/// to the chain; [`GrooveRefusal::ArrowFaceNotAtToe`] when the outer face meets no picked face,
/// more than one, or one that is not an end of the chain. The errors of the ops calls otherwise.
pub fn chain(pick: &GroovePick<'_>, part: JointPart) -> Result<Chain> {
    let faces = picked(pick, part)?;
    // each face's edges, the outer face's last
    let edges =
        faces.iter().chain([&pick.outer]).map(|&f| pick.part.rim_edges(pick.order, &[f])).collect::<solidcraft_kernel::services::Result<Vec<_>>>()?;
    let outer_edges = edges.last().map_or(&[][..], Vec::as_slice);
    let shared = |a: &[usize], b: &[usize]| a.iter().copied().filter(|e| b.contains(e)).collect::<Vec<usize>>();
    // each picked face's picked neighbours, by position in `faces`
    let mut next: Vec<Vec<usize>> = Vec::with_capacity(faces.len());
    for (i, f) in faces.iter().enumerate() {
        let near: Vec<usize> = (0..faces.len()).filter(|&j| j != i && !shared(&edges[i], &edges[j]).is_empty()).collect();
        if near.len() > 2 {
            return Err(GrooveRefusal::PicksNotOneChain { part, face: *f });
        }
        next.push(near);
    }
    // the toe: the one picked face beside the outer face, at an end of the chain (a closed ring
    // of faces has no end, so it lands here too)
    let beside: Vec<usize> = (0..faces.len()).filter(|&i| !shared(&edges[i], outer_edges).is_empty()).collect();
    let start = match beside.as_slice() {
        [i] if next[*i].len() <= 1 => *i,
        _ => return Err(GrooveRefusal::ArrowFaceNotAtToe { part, face: pick.outer }),
    };
    let mut order = vec![start];
    let mut at = start;
    while let Some(&j) = next[at].iter().find(|&&j| !order.contains(&j)) {
        order.push(j);
        at = j;
    }
    if let Some(off) = (0..faces.len()).find(|i| !order.contains(i)) {
        return Err(GrooveRefusal::PicksNotOneChain { part, face: faces[off] });
    }
    Ok(Chain { toe: shared(&edges[start], outer_edges), faces: order.into_iter().map(|i| faces[i]).collect() })
}

/// The picked groove faces of `pick`, sorted, each once.
///
/// # Errors
/// [`GrooveRefusal::Invalid`] for no groove face, or a face past the part's faces;
/// [`GrooveRefusal::TJoint`] when the outer face is among the groove faces.
fn picked(pick: &GroovePick<'_>, part: JointPart) -> Result<Vec<usize>> {
    let mut faces = pick.faces.to_vec();
    faces.sort_unstable();
    faces.dedup();
    if faces.is_empty() {
        return Err(GrooveRefusal::Invalid("no groove face picked"));
    }
    if faces.iter().chain([&pick.outer]).any(|&f| f >= pick.order.faces.len()) {
        return Err(GrooveRefusal::Invalid("a groove or outer face the part does not have"));
    }
    if faces.contains(&pick.outer) {
        return Err(GrooveRefusal::TJoint { part });
    }
    Ok(faces)
}

/// The joint's frame, from both picks and their chains: a plane whose normal is the joint's
/// direction `d`, along which every groove and outer face of both parts runs straight.
///
/// `d` is the direction those faces are proved straight along ([`straight_direction`], ops): the
/// cross product of the two plane normals furthest from parallel when two are proved apart, else
/// a cylinder's axis (a round groove between coplanar plates), each face then proved straight
/// along it within the zone tolerance. Which faces give it depends on their normals only, so the
/// order of the picks and of the parts changes nothing but `d`'s sign.
///
/// The frame (brett19 on #18500): `y` is the left outer face's outward normal with its part along
/// `d` taken out, `x = y × d` ([`affine::frame_axes`], an exact cross product), and `d` takes the
/// sign that makes `x` point out of the left part at its toe: against the direction into the left
/// outer face from its toe edge ([`Body::edge_inward`], read from the face's loop, so an outer face
/// that is not convex is read as a convex one is). Swapping the parts reverses `d`. The origin is
/// the middle of the left toe's first edge.
///
/// # Errors
/// [`GrooveRefusal::OuterFaceNotPlane`]; [`GrooveRefusal::ArrowFaceNotAtToe`] for a chain with no
/// toe edge; [`GrooveRefusal::DirectionUndetermined`]; [`GrooveRefusal::GrooveFaceNotStraight`];
/// [`GrooveRefusal::GrooveFaceNotProved`]; [`GrooveRefusal::Invalid`] for a chain with no face.
/// The errors of the ops calls otherwise.
pub fn joint_frame(picks: [&GroovePick<'_>; 2], chains: [&Chain; 2]) -> Result<Plane> {
    Ok(frame_along(picks, chains)?.0)
}

/// [`joint_frame`] and the joint's direction, the plane's normal (`x × y`).
fn frame_along(picks: [&GroovePick<'_>; 2], chains: [&Chain; 2]) -> Result<(Plane, [f64; 3])> {
    let parts = [JointPart::Left, JointPart::Right];
    let outer = [outer_normal(picks[0], parts[0])?, outer_normal(picks[1], parts[1])?];
    // every face of the joint: left part first, groove faces from the toe, then the outer face
    let mut faces: Vec<(JointPart, &GroovePick<'_>, usize)> = Vec::new();
    for ((pick, chain), part) in picks.into_iter().zip(chains).zip(parts) {
        if chain.faces.is_empty() {
            return Err(GrooveRefusal::Invalid("a chain with no groove face"));
        }
        if chain.toe.is_empty() {
            return Err(GrooveRefusal::ArrowFaceNotAtToe { part, face: pick.outer });
        }
        faces.extend(chain.faces.iter().chain([&pick.outer]).map(|&f| (part, pick, f)));
    }
    let d = direction(&faces)?;
    let [left, _] = picks;
    let Some(&toe) = chains[0].toe.first() else {
        return Err(GrooveRefusal::ArrowFaceNotAtToe { part: JointPart::Left, face: left.outer });
    };
    signed_frame(left, toe, d, outer[0])
}

/// The outward normal of `pick`'s outer face.
///
/// # Errors
/// [`GrooveRefusal::OuterFaceNotPlane`] for an outer face that is not a plane.
fn outer_normal(pick: &GroovePick<'_>, part: JointPart) -> Result<[f64; 3]> {
    match pick.part.face_plane_normal(pick.order, pick.outer)? {
        Some(n) => Ok(n.value),
        None => Err(GrooveRefusal::OuterFaceNotPlane { part, face: pick.outer }),
    }
}

/// Whether face `f` runs straight along `d` within the zone tolerance ([`Body::face_straying`]).
fn straight(pick: &GroovePick<'_>, f: usize, d: [f64; 3], zone: f64) -> Result<bool> {
    Ok(pick.part.face_straying(pick.order, f, d)?.is_some_and(|e| e <= zone))
}

/// The direction every one of `faces` is proved straight along, of either sign: one ops call over
/// both parts' faces ([`straight_direction`]), its refusals named by part and face.
///
/// # Errors
/// [`GrooveRefusal::DirectionUndetermined`], [`GrooveRefusal::GrooveFaceNotStraight`],
/// [`GrooveRefusal::GrooveFaceNotProved`]; the errors of the ops call.
fn direction(faces: &[(JointPart, &GroovePick<'_>, usize)]) -> Result<[f64; 3]> {
    let reads: Vec<(&Body, &Order, usize)> = faces.iter().map(|&(_, pick, f)| (pick.part, pick.order, f)).collect();
    let at = |i: usize| faces.get(i).map(|&(part, _, f)| (part, f)).ok_or(GrooveRefusal::Invalid("a face index ops does not have"));
    match straight_direction(&reads)? {
        Straight::Along { direction, .. } => Ok(direction),
        Straight::Undetermined => Err(GrooveRefusal::DirectionUndetermined),
        Straight::NotStraight { face } => {
            let (part, face) = at(face)?;
            Err(GrooveRefusal::GrooveFaceNotStraight { part, face })
        }
        Straight::Unproved { face } => {
            let (part, face) = at(face)?;
            Err(GrooveRefusal::GrooveFaceNotProved { part, face })
        }
    }
}

/// The joint's frame on the left part's toe edge `toe` (brett19 on #18500): `y` the left outer
/// normal `outer` square to `d`, `x = y × d` ([`affine::frame_axes`]), `d` signed so `x` points
/// out of the left part at its toe, against the direction into the left outer face from `toe`
/// ([`Body::edge_inward`]); its origin the toe edge's middle. The frame and the signed `d`.
fn signed_frame(left: &GroovePick<'_>, toe: usize, d: [f64; 3], outer: [f64; 3]) -> Result<(Plane, [f64; 3])> {
    let [mut x, y] = affine::frame_axes(d, outer)?;
    let mut d = d;
    let inward = left.part.edge_inward(left.order, toe, left.outer)?;
    if affine::distance_along([0.0; 3], x, inward) > 0.0 {
        x = x.map(|v| -v);
        d = d.map(|v| -v);
    }
    let toe = left.part.edge_middle(left.order, toe)?;
    let v = |[a, b, c]: [f64; 3]| Vec3::new(a, b, c);
    Ok((Plane { origin: v(toe), x: v(x), y: v(y) }, d))
}

/// A groove weld: its body and what built it.
#[derive(Debug)]
pub struct GrooveWeld {
    /// The weld's body: the fill swept along the joint over the stretch built.
    pub body: Body,
    /// The joint's frame: the section lies in it, the body runs along its normal. Its origin is
    /// the middle of a toe edge of the left part, which need not be in the stretch built.
    pub frame: Plane,
    /// The section's fill: its region in the frame, its depths, what a caller may report.
    pub fill: ChannelFill,
}

/// A stretch of the joint that keeps one section: from `from` to `to` mm along the joint from
/// its start, in the direction of the joint frame's normal (so swapping the parts reverses it).
/// The joint's start is where the first picked groove face begins along that normal (the lowest
/// end of any groove face of either part), not the frame's origin, which is the middle of a toe
/// edge (brett19's point 9 on #22033).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stretch {
    /// Where the stretch starts (mm from the joint's start).
    pub from: f64,
    /// Where it ends.
    pub to: f64,
}

/// The weld that fills the groove between the two picks all along the joint, as `options` asks.
///
/// The joint runs along the direction all its picked faces are proved straight along
/// ([`joint_frame`]) and is cut at every picked groove face's ends along it (brett19 on #18500).
/// A piece between two cuts keeps one section where, on each part, the groove faces over the
/// whole piece are one chain from the toe ([`chain`]), they and the outer face run straight
/// along the joint ([`Body::face_straying`]), and each face reads the same line or arc
/// at both of its ends ([`planar_edges`]); neighbouring pieces of the same section are one
/// stretch. A joint that is one stretch is built: the channel between the walls filled
/// ([`fill_channel`]) and swept along the joint ([`extrude_planar_regions`]). The depth is
/// measured from the left outer face's level where both outer faces are square to the frame's
/// `y` (a butt joint), else from the chord between the toes. Nothing writes to a part.
///
/// # Errors
/// [`GrooveRefusal::SectionChanges`] with the stretches that keep one section, for a joint that
/// is not one stretch ([`groove_weld_over`] builds one of them); [`GrooveRefusal::Invalid`],
/// [`GrooveRefusal::TJoint`], [`GrooveRefusal::OuterFaceNotPlane`];
/// [`GrooveRefusal::ArrowFaceNotAtToe`] when no left groove face meets the left outer face;
/// [`GrooveRefusal::DirectionUndetermined`]; [`GrooveRefusal::GrooveFaceNotStraight`] for a
/// picked face not straight along the joint or a groove face with no length along it;
/// [`GrooveRefusal::GrooveFaceNotProved`];
/// [`GrooveRefusal::Fill`] with the size that builds. The errors of the ops calls otherwise, a
/// fill's input the kernel refuses among them.
pub fn groove_weld(picks: [&GroovePick<'_>; 2], options: &FillOptions) -> Result<GrooveWeld> {
    let zone = sweep::zone_tolerance()?;
    let joint = cut(picks, zone)?;
    match joint.stretches.as_slice() {
        [(s, walls)] if s.from <= zone && s.to >= joint.end - joint.start - zone => build(picks, &joint, *s, walls, options),
        _ => Err(joint.refusal()),
    }
}

/// [`groove_weld`] over `over` only: a stretch of the joint, or a part of one, that keeps one
/// section (the stretches [`GrooveRefusal::SectionChanges`] names), for the app's "weld 0 to 60
/// mm" (Q56).
///
/// # Errors
/// [`GrooveRefusal::SectionChanges`] for an `over` that is not within one such stretch, or no
/// longer than the zone tolerance; otherwise as [`groove_weld`].
pub fn groove_weld_over(picks: [&GroovePick<'_>; 2], options: &FillOptions, over: Stretch) -> Result<GrooveWeld> {
    let zone = sweep::zone_tolerance()?;
    let joint = cut(picks, zone)?;
    let within = |s: &Stretch| s.from <= over.from + zone && over.to <= s.to + zone && over.to - over.from > zone;
    match joint.stretches.iter().find(|(s, _)| within(s)) {
        Some((_, walls)) => build(picks, &joint, over, walls, options),
        None => Err(joint.refusal()),
    }
}

/// The joint cut at its groove faces' ends along it.
struct Joint {
    /// The joint's frame, on the left part's first toe edge ([`signed_frame`]): every wall is
    /// read in it.
    frame: Plane,
    /// Where the joint starts along the frame's normal, from its origin (mm).
    start: f64,
    /// Where it ends.
    end: f64,
    /// The stretches that keep one section, in order along the joint, each with its two walls.
    stretches: Vec<(Stretch, [Vec<Seg2>; 2])>,
}

impl Joint {
    /// The refusal that names the joint's stretches.
    fn refusal(&self) -> GrooveRefusal {
        GrooveRefusal::SectionChanges { stretches: self.stretches.iter().map(|(s, _)| *s).collect() }
    }
}

/// The joint between the two picks cut at every groove face's ends along it, and its stretches
/// that keep one section (see [`groove_weld`]).
fn cut(picks: [&GroovePick<'_>; 2], zone: f64) -> Result<Joint> {
    let parts = [JointPart::Left, JointPart::Right];
    let faces = [picked(picks[0], parts[0])?, picked(picks[1], parts[1])?];
    let outer = [outer_normal(picks[0], parts[0])?, outer_normal(picks[1], parts[1])?];
    let mut all: Vec<(JointPart, &GroovePick<'_>, usize)> = Vec::new();
    for ((pick, part), f) in picks.into_iter().zip(parts).zip(&faces) {
        all.extend(f.iter().chain([&pick.outer]).map(|&f| (part, pick, f)));
    }
    let d = direction(&all)?;
    let [left, _] = picks;
    let Some(toe) = toe_edge(left, &faces[0])? else {
        return Err(GrooveRefusal::ArrowFaceNotAtToe { part: JointPart::Left, face: left.outer });
    };
    let (frame, d) = signed_frame(left, toe, d, outer[0])?;
    let o = point(frame.origin);

    // each groove face's span along the joint, from its edges across it
    let mut spans: [Vec<(usize, [f64; 2])>; 2] = [Vec::new(), Vec::new()];
    for (((pick, part), f), span) in picks.into_iter().zip(parts).zip(&faces).zip(&mut spans) {
        for &face in f {
            let edges = pick.part.rim_edges(pick.order, &[face])?;
            let read = planar_edges(pick.part, pick.order, &frame, &edges)?;
            let mut at = Vec::new();
            for (&e, piece) in edges.iter().zip(&read.pieces) {
                if piece.is_some() {
                    at.push(affine::distance_along(o, d, pick.part.edge_middle(pick.order, e)?));
                }
            }
            let lo = at.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = at.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if !(hi - lo > zone) {
                return Err(GrooveRefusal::GrooveFaceNotStraight { part, face });
            }
            span.push((face, [lo, hi]));
        }
    }

    // the cuts: every span's ends, those within the zone tolerance of each other as one
    let mut cuts: Vec<f64> = spans.iter().flatten().flat_map(|&(_, s)| s).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|later, earlier| *later - *earlier <= zone);
    let (Some(&start), Some(&end)) = (cuts.first(), cuts.last()) else {
        return Err(GrooveRefusal::Invalid("no groove face picked"));
    };
    // each piece between two cuts that keeps one section; neighbours of one section join
    let tol = zone * 8.0;
    let mut stretches: Vec<(Stretch, [Vec<Seg2>; 2])> = Vec::new();
    let mut joined = false;
    for w in cuts.windows(2) {
        let &[lo, hi] = w else { continue };
        let Some(walls) = piece_walls(picks, &spans, [lo, hi], &frame, d, zone)? else {
            joined = false;
            continue;
        };
        let joins = joined && stretches.last().is_some_and(|(_, last)| same_walls(last, &walls, tol));
        joined = true;
        if !joins {
            stretches.push((Stretch { from: lo - start, to: hi - start }, walls));
        } else if let Some((s, _)) = stretches.last_mut() {
            s.to = hi - start;
        }
    }
    Ok(Joint { frame, start, end, stretches })
}

/// The two walls, read in `frame`, of the piece of the joint from `lo` to `hi` along `d`, where
/// it keeps one section (see [`groove_weld`]); `None` where it does not.
fn piece_walls(
    picks: [&GroovePick<'_>; 2],
    spans: &[Vec<(usize, [f64; 2])>; 2],
    [lo, hi]: [f64; 2],
    frame: &Plane,
    d: [f64; 3],
    zone: f64,
) -> Result<Option<[Vec<Seg2>; 2]>> {
    let parts = [JointPart::Left, JointPart::Right];
    let o = point(frame.origin);
    let mut walls: [Vec<Seg2>; 2] = [Vec::new(), Vec::new()];
    for (((pick, span), part), wall) in picks.into_iter().zip(spans).zip(parts).zip(&mut walls) {
        let faces: Vec<usize> = span.iter().filter(|&&(_, [a, b])| a <= lo + zone && b >= hi - zone).map(|&(f, _)| f).collect();
        if faces.is_empty() {
            return Ok(None);
        }
        let links = match chain(&GroovePick { faces: &faces, ..*pick }, part) {
            Ok(links) => links,
            Err(e @ GrooveRefusal::Ops(_)) => return Err(e),
            Err(_) => return Ok(None),
        };
        let Some(&toe) = links.toe().first() else {
            return Ok(None);
        };
        for &f in links.faces().iter().chain([&pick.outer]) {
            if !straight(pick, f, d, zone)? {
                return Ok(None);
            }
        }
        // the toe in the frame: its edge's middle, along x and y
        let at = pick.part.edge_middle(pick.order, toe)?;
        let toe = Vec2::new(affine::distance_along(o, point(frame.x), at), affine::distance_along(o, point(frame.y), at));
        let Some(read) = wall_of(pick, &links, frame, toe, zone)? else {
            return Ok(None);
        };
        *wall = read;
    }
    Ok(Some(walls))
}

/// The weld over `over`, a stretch (or part of one) whose walls are `walls`.
fn build(picks: [&GroovePick<'_>; 2], joint: &Joint, over: Stretch, walls: &[Vec<Seg2>; 2], options: &FillOptions) -> Result<GrooveWeld> {
    let frame = joint.frame;
    // a butt joint measures the depth from the plates' outer faces, square to the frame's y
    let right = picks[1];
    let inside = right.part.face_centroid(right.order, right.outer, 1e-8)?.centroid;
    let butt = right.part.face_square_level(right.order, right.outer, point(frame.y), inside)?.is_some();
    let depth_normal = butt.then_some(Vec2::new(0.0, 1.0));
    let [left_wall, right_wall] = walls;
    let fill = match fill_channel(left_wall, right_wall, depth_normal, options)? {
        Ok(fill) => fill,
        Err(refusal) => return Err(GrooveRefusal::Fill { why: refusal.why, fit: refusal.fit }),
    };
    let body = extrude_planar_regions(&frame, std::slice::from_ref(&fill.region), joint.start + over.from, joint.start + over.to)?;
    Ok(GrooveWeld { body, frame, fill })
}

/// The first edge a groove face of `pick` (in the order of `faces`) shares with its outer face.
fn toe_edge(pick: &GroovePick<'_>, faces: &[usize]) -> Result<Option<usize>> {
    let outer = pick.part.rim_edges(pick.order, &[pick.outer])?;
    for &f in faces {
        if let Some(&e) = pick.part.rim_edges(pick.order, &[f])?.iter().find(|e| outer.contains(e)) {
            return Ok(Some(e));
        }
    }
    Ok(None)
}

fn point(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// Whether two stretches' walls are the same lines and arcs.
fn same_walls(a: &[Vec<Seg2>; 2], b: &[Vec<Seg2>; 2], tol: f64) -> bool {
    a.iter().zip(b).all(|(p, q)| p.len() == q.len() && p.iter().zip(q).all(|(s, t)| same(s, t, tol)))
}

/// A [`GrooveRefusal::SectionChanges`] in weld words.
fn stretches_text(stretches: &[Stretch]) -> String {
    if stretches.is_empty() {
        return "the groove keeps one section over no stretch of the joint".to_owned();
    }
    let list: Vec<String> = stretches.iter().map(|s| format!("{:.2} to {:.2} mm", s.from, s.to)).collect();
    format!("the groove does not keep one section all along the joint: weld {} along it on their own", list.join(", "))
}

/// What the weld tool tells the user about a groove weld that built ([`GrooveWeld::notes`]).
#[derive(Debug, Clone, PartialEq)]
pub struct WeldNote {
    /// The note in weld words, sizes in mm.
    pub text: String,
    /// A warning: the tool shows it as one and tints the bead (a concave face below a plate's
    /// surface, which fails ISO 5817 at every quality level).
    pub warning: bool,
}

impl GrooveWeld {
    /// The fill's notes in weld words ([`FillNote`] says them in the kernel's).
    #[must_use]
    pub fn notes(&self) -> Vec<WeldNote> {
        self.fill.notes.iter().map(weld_note).collect()
    }
}

/// A fill face in weld words.
fn face_name(side: FillSide) -> &'static str {
    match side {
        FillSide::Face => "weld face",
        FillSide::Second => "root-side weld face",
    }
}

/// One fill note in weld words.
fn weld_note(note: &FillNote) -> WeldNote {
    let (text, warning) = match *note {
        FillNote::DeeperThanChannel { asked, channel } => {
            (format!("a depth of {asked:.2} mm is deeper than the groove ({channel:.2} mm): the whole groove is filled"), false)
        }
        FillNote::DeeperThanOpening { asked, opening } => {
            (format!("a depth of {asked:.2} mm is past the root ({opening:.2} mm): full penetration, the groove is filled through"), false)
        }
        FillNote::BuiltFlat { side, asked } => {
            (format!("the {} asked {asked:.2} mm high or deep is within the model tolerance: built flat", face_name(side)), false)
        }
        FillNote::BelowToes { side, depth, below: [left, right], .. } if left > 0.0 || right > 0.0 => (
            format!(
                "the concave {} dips {depth:.2} mm below the chord, {left:.2} mm below the left plate's surface and {right:.2} mm \
                 below the right one's: an incompletely filled groove (ISO 6520-1 no. 511), which fails ISO 5817 at every \
                 quality level",
                face_name(side)
            ),
            true,
        ),
        FillNote::BelowToes { side, depth, .. } => {
            (format!("the concave {} dips {depth:.2} mm below the chord, not below either plate's surface", face_name(side)), false)
        }
        // shortcut: a note the kernel adds later shows in its own words until it gets weld words
        _ => (format!("{note:?}"), false),
    };
    WeldNote { text, warning }
}

/// Why a fill is refused, in weld words, with the size that builds where there is one.
fn fill_text(why: &FillFailure, fit: Option<f64>) -> String {
    let text = match *why {
        FillFailure::ConcaveFaceCrossesWall { side, depth } => {
            format!("a concave {} {depth:.2} mm deep reaches the groove's sides", face_name(side))
        }
        FillFailure::ConcaveFaceTooDeep { side, depth } => {
            format!("a concave {} {depth:.2} mm deep dips past half a circle across the groove", face_name(side))
        }
        FillFailure::FaceTooShallow { side, size } => {
            format!("a {} {size:.2} mm high or deep is too small to model as an arc and too large to build flat", face_name(side))
        }
        FillFailure::ConvexFaceTooHigh { side, height } => {
            format!("a convex {} {height:.2} mm high rises past half a circle across the groove", face_name(side))
        }
        FillFailure::BlendedToesOffChord { side } => {
            format!("blended toes on the {} need the plate surfaces beside the toes to run level with the line between them", face_name(side))
        }
        _ => why.to_string(),
    };
    match fit {
        Some(f) => format!("the groove cannot be filled as asked: {text}; {f:.2} mm builds"),
        None => format!("the groove cannot be filled as asked: {text}"),
    }
}

/// A part's wall in the joint's frame, from its toe `toe` (in the frame) away from the face: each
/// groove face's section, read from its edges across the joint, which must agree, each starting
/// where the one before ends. `None` where a face reads no one line or arc or does not start
/// there.
fn wall_of(pick: &GroovePick<'_>, chain: &Chain, frame: &Plane, toe: Vec2, zone: f64) -> Result<Option<Vec<Seg2>>> {
    let tol = zone * 8.0;
    let mut wall: Vec<Seg2> = Vec::new();
    let mut at = toe;
    for &face in &chain.faces {
        let edges = pick.part.rim_edges(pick.order, &[face])?;
        let read = planar_edges(pick.part, pick.order, frame, &edges)?;
        let agree = read.tolerance.unwrap_or(0.0).max(zone);
        let reads: Vec<Seg2> = read.pieces.into_iter().flatten().filter_map(|p| one_seg(&p)).collect();
        let seg = match reads.as_slice() {
            [a, b] if same(a, b, agree) => *a,
            _ => return Ok(None),
        };
        let (a, b) = ends(&seg);
        let seg = if near(a, at, tol) {
            seg
        } else if near(b, at, tol) {
            reversed(&seg)
        } else {
            return Ok(None);
        };
        at = ends(&seg).1;
        wall.push(seg);
    }
    Ok(Some(wall))
}

/// The piece of one edge's read when it is a single line or arc.
fn one_seg(pieces: &[PlanarPiece]) -> Option<Seg2> {
    match pieces {
        [PlanarPiece::Seg(s @ (Seg2::Line { .. } | Seg2::Arc { .. }))] => Some(*s),
        _ => None,
    }
}

/// A line's or an arc's two ends. shortcut: an arc's ends by its angles in plain trigonometry,
/// as the app's sheet-metal code reads them; an ops call for it if reviewers ask.
fn ends(s: &Seg2) -> (Vec2, Vec2) {
    match *s {
        Seg2::Arc { center, radius, start, sweep } => {
            let at = |t: f64| Vec2::new(center.x + radius * t.cos(), center.y + radius * t.sin());
            (at(start), at(start + sweep))
        }
        Seg2::Line { a, b } => (a, b),
        _ => (Vec2::new(f64::NAN, f64::NAN), Vec2::new(f64::NAN, f64::NAN)),
    }
}

fn near(p: Vec2, q: Vec2, tol: f64) -> bool {
    (p.x - q.x).abs() <= tol && (p.y - q.y).abs() <= tol
}

/// Whether two reads of a face's ends are the same line or arc, either way round.
fn same(a: &Seg2, b: &Seg2, tol: f64) -> bool {
    let ((a0, a1), (b0, b1)) = (ends(a), ends(b));
    let ends_agree = (near(a0, b0, tol) && near(a1, b1, tol)) || (near(a0, b1, tol) && near(a1, b0, tol));
    let kind_agrees = match (a, b) {
        (Seg2::Line { .. }, Seg2::Line { .. }) => true,
        (Seg2::Arc { center: c, radius: r, .. }, Seg2::Arc { center: k, radius: q, .. }) => near(*c, *k, tol) && (r - q).abs() <= tol,
        _ => false,
    };
    ends_agree && kind_agrees
}

/// The same line or arc the other way round.
fn reversed(s: &Seg2) -> Seg2 {
    match *s {
        Seg2::Line { a, b } => Seg2::Line { a: b, b: a },
        Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center, radius, start: start + sweep, sweep: -sweep },
        other => other,
    }
}

#[cfg(test)]
#[path = "weld_groove_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
