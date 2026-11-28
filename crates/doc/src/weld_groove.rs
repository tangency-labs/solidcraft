//! Groove welds (#18500): the joint of a groove (butt) weld, read from the two parts the user
//! picked: each part's groove faces as one chain from the toe ([`chain`]), the joint's direction
//! and frame ([`joint_frame`]), and the weld that fills the groove ([`groove_weld`]), or one
//! stretch of it that keeps one section ([`groove_weld_over`]).
//!
//! SolidCraft's own: solidcore and solidcore-ops hold no weld (bherbruck on #18409). Every read
//! here is a kernel call through the kernel interface (`solidcraft_kernel`, after #22903); this
//! module adds the weld's rules and refusals. Nothing writes to a part.
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
//! | [`NoGroove`]      | A joint with no groove modelled: its seams, and the mark on one     |
//! | [`Seam`]          | Where the seam runs on one welded side                              |
//! | [`GrooveRefusal`] | Why a joint is refused                                              |
//!
//! # Not covered
//!
//! A T joint's through part has no toe edge: its outer face is its only groove face, and its
//! toe is where the stem's outer face meets it ([`groove_weld`]). A joint with no groove modelled
//! gets a seam and a flat mark ([`NoGroove`]) only between butt plates. A joint whose section
//! changes along it, or whose parts do not run alongside each other all along it, is refused
//! with the stretches that keep one section ([`GrooveRefusal::SectionChanges`], brett19 on
//! #18500, Q56); each one builds on its own ([`groove_weld_over`]). Building them all at once
//! comes later. [`joint_frame`] reads one chain over the whole joint, so a groove face split
//! along the joint into faces side by side is refused there ([`GrooveRefusal::ArrowFaceNotAtToe`]);
//! [`groove_weld`] reads the chain piece by piece (brett19's point 7 on #22033). An outer face
//! split along the joint into faces side by side is not covered: one outer face is picked, so
//! the toe is read only where that face meets the groove. Faces that lie on one surface are read
//! as separate faces.
//!
//! A groove face whose end is not square to the joint (a plate end cut at an angle) is read where
//! its whole section is present ([`groove_weld`]): past that the section shrinks, so the joint is
//! refused with the stretch where every groove face is whole, which builds on its own, as for
//! parts of different length (brett19's point 1 on #22033 review 5). A weld whose end follows the
//! plate's end comes later. A J or U face reads its arc from an end square to the joint: one whose
//! ends are both cut at an angle gives no section.

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel::affine;
use solidcraft_kernel::api::UnitNormal;
use solidcraft_kernel::{
    Body, ChannelFill, FillFailure, FillNote, FillOptions, FillSide, KernelError, PlanarPiece, Straight, extrude_planar_regions, fill_channel,
    profiles, straight_direction,
};

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
    /// A part's outer face is one of several groove faces picked on it: a T joint's through part
    /// has its outer face as its only groove face. [`chain`] refuses any outer face among the
    /// groove faces: a through part has no chain.
    #[error("the {part:?} part's outer face is one of its groove faces (a T joint's through part picks its outer face alone)")]
    TJoint { part: JointPart },
    /// A T joint whose stem (`part`) does not stand on the through part's outer face: the stem's
    /// outer face does not meet that face ahead of the stem's toe, or the stem's groove does not
    /// end on it (a stem standing off it). Not built.
    #[error("the {part:?} part's groove does not end on the through part's outer face")]
    StemOffThroughPart { part: JointPart },
    /// A part's outer face `face` is not a plane.
    #[error("the {part:?} part's outer face {face} is not a plane")]
    OuterFaceNotPlane { part: JointPart, face: usize },
    /// The second part's outer face `face` is not on the arrow side the first one's sets: it
    /// neither faces within 45° of the first outer face's side and runs away from the first part
    /// (a butt), nor faces within 45° of straight out of the joint and runs away from the arrow
    /// side (a corner), each by more than [`same_side`]'s margin: a face turned back over the
    /// groove or into the joint, or one too near 45° to tell (brett19's point A on #22033
    /// review 6).
    #[error(
        "the {part:?} part's outer face {face} is not on the arrow side of the joint: turned back over the groove or into it, or too near 45° to tell"
    )]
    OuterFaceWrongSide { part: JointPart, face: usize },
    /// Face `face` of a part (a groove face or its outer face) strays across the joint's direction
    /// by more than the zone tolerance ([`Body::face_straying`]): it does not run straight along
    /// the joint. From the direction search, no direction runs every face straight, and the face
    /// named is the first, in the order the faces are read (the left part's, then the right one's,
    /// each part's outer face last), that strays from the first direction tried. [`groove_weld`]
    /// also names a groove face with no length along the joint.
    #[error("face {face} of the {part:?} part is not straight along the joint")]
    GrooveFaceNotStraight { part: JointPart, face: usize },
    /// Groove face `face` of a part is never whole across the joint: its runs along the joint
    /// ([`face_read`]) overlap by no more than the zone tolerance (a plate whose ends lean past
    /// each other), so no stretch has its whole section (brett19's point D on #22033 review 6).
    #[error("face {face} of the {part:?} part has its whole section nowhere along the joint: its ends lean past each other")]
    GrooveFaceNeverWhole { part: JointPart, face: usize },
    /// Groove face `face` of a part is interrupted inside its span along the joint: a hole or
    /// cut-out inside it (a second loop), or a notch in one of its edges along the joint (a run
    /// with a gap, [`face_read`]), so its section is not whole all along (CodeRabbit on #22033).
    #[error("face {face} of the {part:?} part has a hole, cut-out or notch: its section is not whole all along the joint")]
    GrooveFaceInterrupted { part: JointPart, face: usize },
    /// Face `face` of a part (a groove face or its outer face) is a surface whose straightness
    /// along the joint the kernel cannot prove (no bound from [`Body::face_straying`]): it may be
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
    /// No groove is modelled: the two walls coincide within the zone tolerance (brett19 on
    /// #18500). Its own outcome, not a failure: the weld tool draws a seam line and a flat mark
    /// ([`NoGroove::mark`]) instead of a body that fills a groove.
    #[error("no groove is modelled between the parts: a seam and a mark, no fill")]
    NoGroove(Box<NoGroove>),
    /// The walls are apart, but nowhere by more than `gap` (mm), within the zone tolerance plus
    /// the fill's depth margin: a sliver too thin to fill or to call no groove (brett19 on #18500).
    #[error("the groove is a sliver {gap:.6} mm wide: too thin to fill")]
    Sliver { gap: f64 },
    /// The search for a direction the faces all run straight along was not decided (the kernel's
    /// [`Straight::Undecided`]: too many faces for its budget, or bands it cannot reach): the
    /// joint may run straight, but no direction is proved. Not a fault of the picks.
    #[error("the joint's direction could not be decided for these faces")]
    DirectionUndecided,
    /// A kernel call failed.
    #[error(transparent)]
    Kernel(#[from] KernelError),
    /// A placement call refused its numbers, in the kernel's words.
    #[error(transparent)]
    Placement(#[from] affine::AffineRefused),
}

type Result<T> = std::result::Result<T, GrooveRefusal>;

/// One part's side of a groove joint.
#[derive(Debug, Clone, Copy)]
pub struct GroovePick<'a> {
    /// The part (only read); the faces below are its record face indices.
    pub part: &'a Body,
    /// Its groove faces (bevel, root face, J or U radius, square wall), in any order.
    pub faces: &'a [usize],
    /// Its plate face on the arrow side: the side the weld is made from.
    pub outer: usize,
}

/// A part's groove faces in order from the toe. Only [`chain`] makes one.
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
/// more than one, or one that is not an end of the chain. The errors of the kernel calls otherwise.
pub fn chain(pick: &GroovePick<'_>, part: JointPart) -> Result<Chain> {
    let faces = picked(pick, part)?;
    // each face's edges, the outer face's last
    let edges = faces.iter().chain([&pick.outer]).map(|&f| pick.part.rim_edges(&[f])).collect::<solidcraft_kernel::Result<Vec<_>>>()?;
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
    if faces.iter().chain([&pick.outer]).any(|&f| f >= pick.part.record().faces.len()) {
        return Err(GrooveRefusal::Invalid("a groove or outer face the part does not have"));
    }
    if faces.contains(&pick.outer) {
        return Err(GrooveRefusal::TJoint { part });
    }
    Ok(faces)
}

/// The joint's frame, from both picks: a plane whose normal is the joint's direction `d`, along
/// which every groove and outer face of both parts runs straight. Each pick's groove faces are
/// read as one chain from the toe first ([`chain`]).
///
/// `d` is the direction those faces are proved straight along ([`straight_direction`], the kernel's), the
/// first of these that proves every face straight within the zone tolerance: the cross product of
/// the two plane normals furthest from parallel when two are proved apart; else a direction inside
/// every plane's and cylinder's band (how far `d` may lean for the face to stay within the zone);
/// else a cylinder's axis (a round groove between coplanar plates). Which faces give it depends on
/// their normals only, so the order of the picks and of the parts changes nothing but `d`'s sign.
///
/// The frame (brett19 on #18500): `y` is the left outer face's outward normal with its part along
/// `d` taken out, `x = y × d` ([`affine::frame_axes`], an exact cross product), and `d` takes the
/// sign that makes `x` point out of the left part at its toe: against the direction into the left
/// outer face from its toe edge ([`Body::edge_inward`], read from the face's loop, so an outer face
/// that is not convex is read as a convex one is). Swapping the parts reverses `d`. The origin is
/// the middle of the left toe's first edge. The right outer face must lie on the same side: facing
/// within 45° of `y` and running from its toe within 45° of `x`, away from the left part (a butt),
/// or facing within 45° of `x` and running from its toe within 45° of `-y`, away from the arrow
/// side (a corner), each by more than a margin ([`same_side`]). The 45° keeps a face square to the
/// left one off the line the rounding decides (brett19's point 3 on #22033 review 5): a square
/// corner is decided by components near 1; the margin keeps a face at 45° off it (his point A on
/// review 6).
///
/// # Errors
/// The errors of [`chain`] for either pick; [`GrooveRefusal::OuterFaceNotPlane`];
/// [`GrooveRefusal::DirectionUndetermined`]; [`GrooveRefusal::DirectionUndecided`];
/// [`GrooveRefusal::GrooveFaceNotStraight`]; [`GrooveRefusal::GrooveFaceNotProved`];
/// [`GrooveRefusal::OuterFaceWrongSide`]. The errors of the kernel calls otherwise.
pub fn joint_frame(picks: [&GroovePick<'_>; 2]) -> Result<Plane> {
    let parts = [JointPart::Left, JointPart::Right];
    let chains = [chain(picks[0], parts[0])?, chain(picks[1], parts[1])?];
    let outer = [outer_normal(picks[0], parts[0])?, outer_normal(picks[1], parts[1])?];
    // every face of the joint: left part first, groove faces from the toe, then the outer face
    let mut faces: Vec<(JointPart, &GroovePick<'_>, usize)> = Vec::new();
    for ((pick, chain), part) in picks.into_iter().zip(&chains).zip(parts) {
        faces.extend(chain.faces.iter().chain([&pick.outer]).map(|&f| (part, pick, f)));
    }
    let d = direction(&faces)?;
    let [left, right] = picks;
    // chain() gives a toe edge on every chain it makes
    let (Some(&toe), Some(&right_toe)) = (chains[0].toe.first(), chains[1].toe.first()) else {
        return Err(GrooveRefusal::Invalid("a chain with no toe edge"));
    };
    let (frame, _) = signed_frame(left, toe, d, outer[0].value)?;
    let margin = side_margin(picks, &outer, zone_tolerance()?)?;
    same_side(right, JointPart::Right, right_toe, &frame, outer[1].value, margin)?;
    Ok(frame)
}

/// The outward normal of `pick`'s outer face, with its proved bound.
///
/// # Errors
/// [`GrooveRefusal::OuterFaceNotPlane`] for an outer face that is not a plane.
fn outer_normal(pick: &GroovePick<'_>, part: JointPart) -> Result<UnitNormal> {
    match pick.part.face_plane_normal(pick.outer)? {
        Some(n) => Ok(n),
        None => Err(GrooveRefusal::OuterFaceNotPlane { part, face: pick.outer }),
    }
}

/// How far either side of 45° [`same_side`] refuses rather than decides (brett19's point A on
/// #22033 review 6): the two outer normals' proved bounds ([`Body::face_plane_normal`]), the
/// product of the two outer faces' band half widths for the zone tolerance `zone`
/// ([`Body::face_plane_band`]: the joint's direction may lean inside them, which moves `y · outer` by up
/// to that product), and a few ulps for the comparisons' own rounding. shortcut: the direction into
/// the right outer face from its toe is rounded, not certified ([`Body::edge_inward`]); a proved
/// bound for it if a face this near 45° comes up.
///
/// # Errors
/// [`GrooveRefusal::OuterFaceNotPlane`] for an outer face the kernel gives no plane band; the
/// errors of the kernel calls.
fn side_margin(picks: [&GroovePick<'_>; 2], outer: &[UnitNormal; 2], zone: f64) -> Result<f64> {
    let mut slopes = [0.0; 2];
    for ((pick, part), slope) in picks.into_iter().zip([JointPart::Left, JointPart::Right]).zip(&mut slopes) {
        match pick.part.face_plane_band(pick.outer, zone)? {
            Some(s) => *slope = s,
            None => return Err(GrooveRefusal::OuterFaceNotPlane { part, face: pick.outer }),
        }
    }
    Ok(outer[0].bound + outer[1].bound + slopes[0] * slopes[1] + 8.0 * f64::EPSILON)
}

/// Whether face `f` runs straight along `d` within the zone tolerance ([`Body::face_straying`]).
fn straight(pick: &GroovePick<'_>, f: usize, d: [f64; 3], zone: f64) -> Result<bool> {
    Ok(pick.part.face_straying(f, d)?.is_some_and(|e| e <= zone))
}

/// The direction every one of `faces` is proved straight along, of either sign: one kernel call over
/// both parts' faces ([`straight_direction`]), its refusals named by part and face.
///
/// # Errors
/// [`GrooveRefusal::DirectionUndetermined`], [`GrooveRefusal::DirectionUndecided`],
/// [`GrooveRefusal::GrooveFaceNotStraight`], [`GrooveRefusal::GrooveFaceNotProved`]; the errors of
/// the kernel call.
fn direction(faces: &[(JointPart, &GroovePick<'_>, usize)]) -> Result<[f64; 3]> {
    let reads: Vec<(&Body, usize)> = faces.iter().map(|&(_, pick, f)| (pick.part, f)).collect();
    let at = |i: usize| faces.get(i).map(|&(part, _, f)| (part, f)).ok_or(GrooveRefusal::Invalid("a face index the kernel does not have"));
    match straight_direction(&reads)? {
        Straight::Along { direction, .. } => Ok(direction),
        Straight::Undetermined => Err(GrooveRefusal::DirectionUndetermined),
        Straight::Undecided => Err(GrooveRefusal::DirectionUndecided),
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
    let inward = left.part.edge_inward(toe, left.outer)?;
    if affine::distance_along([0.0; 3], x, inward) > 0.0 {
        x = x.map(|v| -v);
        d = d.map(|v| -v);
    }
    let toe = edge_middle(left.part, toe)?;
    let v = |[a, b, c]: [f64; 3]| Vec3::new(a, b, c);
    Ok((Plane { origin: v(toe), x: v(x), y: v(y) }, d))
}

/// Whether the second part's outer face, of outward normal `outer`, lies on the arrow side the
/// first one sets in `frame`, as at a butt joint or at a corner. A butt: facing nearer its `y`
/// than across the joint, and running from the toe edge `toe` nearer its `x` than its `y`, away
/// from the first part. A corner: facing nearer its `x` (out of the joint) than its `y`, and
/// running from the toe nearer its `-y` than its `x`, away from the arrow side
/// ([`Body::edge_inward`]). Each by more than `margin` ([`side_margin`]), and each a comparison of
/// two components of one unit vector, so a face square to the first one is decided by a component
/// near 1 against one near 0, and one at 45° is refused, whatever the rounding.
///
/// # Errors
/// [`GrooveRefusal::OuterFaceWrongSide`] naming `part`; the errors of the kernel calls.
fn same_side(right: &GroovePick<'_>, part: JointPart, toe: usize, frame: &Plane, outer: [f64; 3], margin: f64) -> Result<()> {
    let inward = right.part.edge_inward(toe, right.outer)?;
    let along = |axis: Vec3, v: [f64; 3]| affine::distance_along([0.0; 3], point(axis), v);
    let butt = along(frame.y, outer) > along(frame.x, outer).abs() + margin && along(frame.x, inward) > along(frame.y, inward).abs() + margin;
    let corner = along(frame.x, outer) > along(frame.y, outer).abs() + margin && -along(frame.y, inward) > along(frame.x, inward).abs() + margin;
    if butt || corner { Ok(()) } else { Err(GrooveRefusal::OuterFaceWrongSide { part, face: right.outer }) }
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
    /// The zone tolerance it was built to: a dip below a plate's surface no deeper is no dip.
    zone: f64,
}

/// A stretch of the joint that keeps one section: from `from` to `to` mm along the joint from
/// its start, in the direction of the joint frame's normal (so swapping the parts reverses it).
/// The joint's start is where the first picked groove face's whole section begins along that
/// normal (the lowest such start of any groove face of either part), not the frame's origin, which
/// is the middle of a toe edge (brett19's point 9 on #22033).
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
/// ([`joint_frame`]) and is cut where each picked groove face's whole section starts and ends
/// along it (brett19 on #18500; [`face_read`]: an end not square to the joint is cut where the
/// face is whole). A piece between two cuts keeps one section where, on each part, the groove
/// faces over the whole piece are one chain from the toe ([`chain`]), they and the outer face run
/// straight along the joint ([`Body::face_straying`]), and each face reads one line or arc across
/// it ([`face_read`]); neighbouring pieces of the same section are one stretch. Where no piece
/// keeps one section because its faces are not one chain from the toe, that refusal is returned.
/// A joint that is one stretch is built: the channel between the walls filled
/// ([`fill_channel`]) and swept along the joint ([`extrude_planar_regions`]). The depth is
/// measured from the left outer face's level where both outer faces are square to the frame's
/// `y` (a butt joint), else from the chord between the toes. Nothing writes to a part.
///
/// A wall whose last face is a plane that runs on past the other wall's end (an upright plate's
/// side at a corner joint) ends where that end lies on it. A part whose outer face is its only
/// groove face is a T joint's through part (brett19 on #18500): its toe is where the other
/// part's (the stem's) outer face meets that face, found by a ray from the stem's toe along the
/// frame's `x`, and its wall runs along that face from the toe to the stem's root, which must
/// lie on it. The frame is then the stem's, whichever part was passed first, and the through
/// part only counts where it runs alongside the stem.
///
/// # Errors
/// [`GrooveRefusal::SectionChanges`] with the stretches that keep one section, for a joint that
/// is not one stretch ([`groove_weld_over`] builds one of them); the errors of [`chain`] where no
/// piece keeps one section and the faces of one are not one chain; [`GrooveRefusal::Invalid`]
/// (both parts through parts among them), [`GrooveRefusal::TJoint`],
/// [`GrooveRefusal::StemOffThroughPart`], [`GrooveRefusal::OuterFaceNotPlane`];
/// [`GrooveRefusal::ArrowFaceNotAtToe`] when no groove face of a part meets its outer face (of the
/// stem, in a T joint); [`GrooveRefusal::OuterFaceWrongSide`];
/// [`GrooveRefusal::DirectionUndetermined`]; [`GrooveRefusal::DirectionUndecided`];
/// [`GrooveRefusal::GrooveFaceNotStraight`] for a picked face not straight along the joint or a groove face with no length along it;
/// [`GrooveRefusal::GrooveFaceNeverWhole`]; [`GrooveRefusal::GrooveFaceInterrupted`];
/// [`GrooveRefusal::GrooveFaceNotProved`];
/// [`GrooveRefusal::Fill`] with the size that builds; [`GrooveRefusal::NoGroove`] (an outcome:
/// a seam and a mark, [`NoGroove::mark`]) where the walls coincide between butt plates;
/// [`GrooveRefusal::Sliver`] where they are only just apart. The errors of the kernel calls otherwise, a
/// fill's input the kernel refuses among them.
pub fn groove_weld(picks: [&GroovePick<'_>; 2], options: &FillOptions) -> Result<GrooveWeld> {
    let zone = zone_tolerance()?;
    let (picks, parts, through) = stem_first(picks)?;
    let joint = cut(picks, parts, through, zone)?;
    match joint.stretches.as_slice() {
        [(s, walls)] if s.from <= zone && s.to >= joint.end - joint.start - zone => build(picks, &joint, *s, walls, options, zone),
        _ => Err(joint.refusal()),
    }
}

/// [`groove_weld`] over `over` only: a stretch of the joint, or a part of one, that keeps one
/// section (the stretches [`GrooveRefusal::SectionChanges`] names), for the app's "weld 0 to 60
/// mm" (Q56).
///
/// An `over` that reaches past its stretch by no more than the zone tolerance is held to the
/// stretch, so the weld never runs past the plates' faces (CodeRabbit on #22033).
///
/// # Errors
/// [`GrooveRefusal::SectionChanges`] for an `over` that is not within one such stretch, or, held
/// to it, no longer than the zone tolerance; otherwise as [`groove_weld`].
pub fn groove_weld_over(picks: [&GroovePick<'_>; 2], options: &FillOptions, over: Stretch) -> Result<GrooveWeld> {
    let zone = zone_tolerance()?;
    let (picks, parts, through) = stem_first(picks)?;
    let joint = cut(picks, parts, through, zone)?;
    let within = |s: &Stretch| s.from <= over.from + zone && over.to <= s.to + zone;
    match joint.stretches.iter().find(|(s, _)| within(s)) {
        Some((s, walls)) => {
            let held = Stretch { from: over.from.max(s.from), to: over.to.min(s.to) };
            if held.to - held.from > zone { build(picks, &joint, held, walls, options, zone) } else { Err(joint.refusal()) }
        }
        None => Err(joint.refusal()),
    }
}

/// The picks with a T joint's stem first, their parts as passed, and whether the second is a
/// T joint's through part (its outer face its only groove face).
///
/// # Errors
/// [`GrooveRefusal::Invalid`] when both are.
fn stem_first<'p, 'a>(picks: [&'p GroovePick<'a>; 2]) -> Result<([&'p GroovePick<'a>; 2], [JointPart; 2], bool)> {
    let through = |p: &GroovePick<'_>| !p.faces.is_empty() && p.faces.iter().all(|&f| f == p.outer);
    let [left, right] = picks;
    match (through(left), through(right)) {
        (false, t) => Ok((picks, [JointPart::Left, JointPart::Right], t)),
        (true, false) => Ok(([right, left], [JointPart::Right, JointPart::Left], true)),
        (true, true) => Err(GrooveRefusal::Invalid("both parts' only groove face is their outer face: no groove on either")),
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

/// The joint between the two picks cut where each groove face's whole section starts and ends
/// along it ([`face_read`]), and its stretches that keep one section (see [`groove_weld`]). With
/// `through`, the second part is a T joint's through part: its outer face is read in place of
/// groove faces, and only where it runs alongside the stem.
fn cut(picks: [&GroovePick<'_>; 2], parts: [JointPart; 2], through: bool, zone: f64) -> Result<Joint> {
    let second = if through {
        if picks[1].outer >= picks[1].part.record().faces.len() {
            return Err(GrooveRefusal::Invalid("a groove or outer face the part does not have"));
        }
        vec![picks[1].outer]
    } else {
        picked(picks[1], parts[1])?
    };
    let faces = [picked(picks[0], parts[0])?, second];
    let outer = [outer_normal(picks[0], parts[0])?, outer_normal(picks[1], parts[1])?];
    let mut all: Vec<(JointPart, &GroovePick<'_>, usize)> = Vec::new();
    for ((pick, part), f) in picks.into_iter().zip(parts).zip(&faces) {
        all.extend(f.iter().chain([&pick.outer]).map(|&f| (part, pick, f)));
    }
    let d = direction(&all)?;
    let [left, _] = picks;
    let Some(toe) = toe_edge(left, &faces[0])? else {
        return Err(GrooveRefusal::ArrowFaceNotAtToe { part: parts[0], face: left.outer });
    };
    let (frame, d) = signed_frame(left, toe, d, outer[0].value)?;
    // a T's through part is found by the ray from the stem's toe ([`through_wall`])
    if !through {
        let right = picks[1];
        let Some(right_toe) = toe_edge(right, &faces[1])? else {
            return Err(GrooveRefusal::ArrowFaceNotAtToe { part: parts[1], face: right.outer });
        };
        same_side(right, parts[1], right_toe, &frame, outer[1].value, side_margin(picks, &outer, zone)?)?;
    }

    // each groove face's span along the joint and its section, read from its own edges
    let mut reads: [Vec<FaceRead>; 2] = [Vec::new(), Vec::new()];
    for (((pick, part), f), read) in picks.into_iter().zip(parts).zip(&faces).zip(&mut reads) {
        for &face in f {
            match face_read(pick, part, face, &frame, d, zone)? {
                Some(r) if r.span[1] - r.span[0] > zone => read.push(r),
                Some(_) => return Err(GrooveRefusal::GrooveFaceNeverWhole { part, face }),
                None => return Err(GrooveRefusal::GrooveFaceNotStraight { part, face }),
            }
        }
    }
    if through {
        // a T joint's through part may run on past the stem: only where it runs alongside counts
        let lo = reads[0].iter().map(|r| r.span[0]).fold(f64::INFINITY, f64::min);
        let hi = reads[0].iter().map(|r| r.span[1]).fold(f64::NEG_INFINITY, f64::max);
        for r in &mut reads[1] {
            r.span[0] = r.span[0].max(lo);
            r.span[1] = r.span[1].min(hi).max(r.span[0]);
        }
    }

    // the cuts: every span's ends, those within the zone tolerance of each other as one
    let mut cuts: Vec<f64> = reads.iter().flatten().flat_map(|r| r.span).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|later, earlier| *later - *earlier <= zone);
    let (Some(&start), Some(&end)) = (cuts.first(), cuts.last()) else {
        return Err(GrooveRefusal::Invalid("no groove face picked"));
    };
    // each piece between two cuts that keeps one section; neighbours of one section join
    let tol = zone * ALLOWANCE;
    let mut stretches: Vec<(Stretch, [Vec<Seg2>; 2])> = Vec::new();
    let mut joined = false;
    let mut why = None;
    for w in cuts.windows(2) {
        let &[lo, hi] = w else { continue };
        let piece = Piece { frame: &frame, d, along: [lo, hi], zone };
        let Some(walls) = piece_walls(picks, parts, through, &reads, &piece, &mut why)? else {
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
    // a joint with no stretch says why where a piece's faces were not one chain (brett19's
    // point 5 on #22033 review 5)
    if let (true, Some(why)) = (stretches.is_empty(), why) {
        return Err(why);
    }
    Ok(Joint { frame, start, end, stretches })
}

/// A piece of the joint: from `along[0]` to `along[1]` along `d`, measured from the frame's
/// origin, every wall read in `frame`.
struct Piece<'f> {
    /// The joint's frame.
    frame: &'f Plane,
    /// The joint's direction, the frame's normal.
    d: [f64; 3],
    /// Where the piece starts and ends along `d`.
    along: [f64; 2],
    /// The kernel's zone tolerance.
    zone: f64,
}

impl Piece<'_> {
    /// The point `p` of the frame's plane (along its `x` and `y`) moved along `d` to the piece's
    /// middle.
    fn in_space(&self, p: Vec2) -> [f64; 3] {
        let [lo, hi] = self.along;
        let on = affine::parallelogram_point(point(self.frame.origin), self.d, point(self.frame.x), (lo + hi) / 2.0, p.x);
        affine::parallelogram_point(on, point(self.frame.y), self.d, p.y, 0.0)
    }

    /// The point `p` in the frame's plane: how far it stands from the origin along `x` and `y`.
    fn in_frame(&self, p: [f64; 3]) -> Vec2 {
        let o = point(self.frame.origin);
        Vec2::new(affine::distance_along(o, point(self.frame.x), p), affine::distance_along(o, point(self.frame.y), p))
    }
}

/// The two walls of `piece`, where it keeps one section (see [`groove_weld`]); `None` where it
/// does not. Where that is because a part's faces in the piece are not one chain from the toe,
/// `why` keeps the first such refusal. With `through`, the second part is a T joint's through
/// part ([`through_wall`]). A wall that runs on past the other's end along a plane ends there
/// ([`ended_at`]).
///
/// # Errors
/// [`GrooveRefusal::StemOffThroughPart`]; the errors of the kernel calls.
fn piece_walls(
    picks: [&GroovePick<'_>; 2],
    parts: [JointPart; 2],
    through: bool,
    reads: &[Vec<FaceRead>; 2],
    piece: &Piece<'_>,
    why: &mut Option<GrooveRefusal>,
) -> Result<Option<[Vec<Seg2>; 2]>> {
    let Some((mut left, left_face)) = part_wall(picks[0], parts[0], &reads[0], piece, why)? else {
        return Ok(None);
    };
    let ([lo, hi], zone) = (piece.along, piece.zone);
    let (mut right, right_face) = if through {
        if !reads[1].iter().any(|r| r.span[0] <= lo + zone && r.span[1] >= hi - zone) {
            return Ok(None);
        }
        match through_wall(picks[1], parts[0], &left, piece)? {
            Some(wall) => (wall, picks[1].outer),
            None => return Ok(None),
        }
    } else {
        match part_wall(picks[1], parts[1], &reads[1], piece, why)? {
            Some(wall) => wall,
            None => return Ok(None),
        }
    };
    let root = |w: &[Seg2]| w.last().map(|s| ends(s).1);
    if let (Some(l), Some(r)) = (root(&left), root(&right))
        && !near(l, r, zone * ALLOWANCE)
    {
        if let Some(ended) = ended_at(picks[1], right_face, &right, l, piece)? {
            right = ended;
        } else if let Some(ended) = ended_at(picks[0], left_face, &left, r, piece)? {
            left = ended;
        }
    }
    Ok(Some([left, right]))
}

/// One part's wall in `piece`, and its last groove face: its groove faces over the whole piece
/// (from `reads`) as one chain from the toe, every one of them and the outer face straight along
/// the joint, each read as one line or arc ([`wall_of`]); `None` where they are not. Where they
/// are not one chain, `why` keeps the first such refusal.
fn part_wall(
    pick: &GroovePick<'_>,
    part: JointPart,
    reads: &[FaceRead],
    piece: &Piece<'_>,
    why: &mut Option<GrooveRefusal>,
) -> Result<Option<(Vec<Seg2>, usize)>> {
    let ([lo, hi], zone) = (piece.along, piece.zone);
    let faces: Vec<usize> = reads.iter().filter(|r| r.span[0] <= lo + zone && r.span[1] >= hi - zone).map(|r| r.face).collect();
    if faces.is_empty() {
        return Ok(None);
    }
    let links = match chain(&GroovePick { faces: &faces, ..*pick }, part) {
        Ok(links) => links,
        Err(e @ GrooveRefusal::Kernel(_)) => return Err(e),
        Err(e) => {
            why.get_or_insert(e);
            return Ok(None);
        }
    };
    let (Some(&last), Some(&toe)) = (links.faces().last(), links.toe().first()) else {
        return Ok(None);
    };
    for &f in links.faces().iter().chain([&pick.outer]) {
        if !straight(pick, f, piece.d, zone)? {
            return Ok(None);
        }
    }
    // the toe in the frame: its edge's middle, along x and y
    let toe = piece.in_frame(edge_middle(pick.part, toe)?);
    Ok(wall_of(reads, &links, toe, zone).map(|wall| (wall, last)))
}

/// A T joint's through part's wall in `piece`, beside the stem's wall `stem`: along its outer
/// face (straight along the joint) from its toe, where the stem's outer face meets it (the
/// first face a ray from the stem's toe along the frame's `x` meets), to the stem's root, which
/// must lie on that face. `None` where the outer face is not straight along the joint.
///
/// # Errors
/// [`GrooveRefusal::StemOffThroughPart`] naming the stem `part` where the ray meets no face, or
/// another face first, or the stem's root is not on the outer face; the errors of the kernel calls.
fn through_wall(pick: &GroovePick<'_>, part: JointPart, stem: &[Seg2], piece: &Piece<'_>) -> Result<Option<Vec<Seg2>>> {
    let tol = piece.zone * ALLOWANCE;
    let off = GrooveRefusal::StemOffThroughPart { part };
    if !straight(pick, pick.outer, piece.d, piece.zone)? {
        return Ok(None);
    }
    let (Some(first), Some(last)) = (stem.first(), stem.last()) else {
        return Err(off);
    };
    let hits = pick.part.face_hits_by_ray(piece.in_space(ends(first).0), point(piece.frame.x))?;
    let Some(hit) = hits.first().filter(|h| h.face == pick.outer) else {
        return Err(off);
    };
    let (toe, root) = (piece.in_frame(hit.point), ends(last).1);
    if near(toe, root, tol) || !on_face(pick, pick.outer, piece.in_space(root), tol)? {
        return Err(off);
    }
    Ok(Some(vec![Seg2::Line { a: toe, b: root }]))
}

/// `wall` ended at `root`, the other wall's end, where its last piece is a line along `face`, a
/// plane, and `root` lies on that face short of the line's end: a plain plate face that runs on
/// past the groove's root, as an upright plate's side does at a corner joint. `None` otherwise.
fn ended_at(pick: &GroovePick<'_>, face: usize, wall: &[Seg2], root: Vec2, piece: &Piece<'_>) -> Result<Option<Vec<Seg2>>> {
    let tol = piece.zone * ALLOWANCE;
    let Some((&Seg2::Line { a, b }, before)) = wall.split_last() else {
        return Ok(None);
    };
    let plane = pick.part.face_plane_normal(face)?.is_some();
    if !plane || near(a, root, tol) || near(b, root, tol) || !on_face(pick, face, piece.in_space(root), tol)? {
        return Ok(None);
    }
    Ok(Some(before.iter().copied().chain([Seg2::Line { a, b: root }]).collect()))
}

/// Whether point `p` lies on record face `face` of `pick`'s part, within `tol`.
fn on_face(pick: &GroovePick<'_>, face: usize, p: [f64; 3], tol: f64) -> Result<bool> {
    Ok(pick.part.closest_to_point(&[face], &[], p)?.is_some_and(|c| c.distance <= tol))
}

/// The weld over `over`, a stretch (or part of one) whose walls are `walls`.
fn build(picks: [&GroovePick<'_>; 2], joint: &Joint, over: Stretch, walls: &[Vec<Seg2>; 2], options: &FillOptions, zone: f64) -> Result<GrooveWeld> {
    let frame = joint.frame;
    // a butt joint measures the depth from the plates' outer faces, square to the frame's y (only
    // whether the right one is square is read: its level, and so the origin, does not matter)
    let right = picks[1];
    let butt = right.part.face_square_level(right.outer, frame.y, frame.origin)?.is_some();
    let depth_normal = butt.then_some(Vec2::new(0.0, 1.0));
    let [left_wall, right_wall] = walls;
    let along = [joint.start + over.from, joint.start + over.to];
    // shortcut: no groove only between butt plates (the far side's seam faces -y); coincident
    // walls elsewhere get the fill's own refusal until a corner or T without a groove is asked for
    if let Some(gap) = apart(left_wall, right_wall) {
        if gap <= zone {
            if let (true, Some(first), Some(last)) = (butt, left_wall.first(), left_wall.last()) {
                let (toe, root) = (ends(first).0, ends(last).1);
                let thickness = toe.y - root.y;
                let seams = [Seam { at: toe, normal: Vec2::new(0.0, 1.0), thickness }, Seam { at: root, normal: Vec2::new(0.0, -1.0), thickness }];
                return Err(GrooveRefusal::NoGroove(Box::new(NoGroove { frame, along, seams })));
            }
        } else if gap <= zone + zone.max(1e-9 * size(left_wall.iter().chain(right_wall))) {
            // within the zone tolerance plus the fill's depth margin (the larger of the
            // section's allowance and the zone tolerance)
            return Err(GrooveRefusal::Sliver { gap });
        }
    }
    let fill = match fill_channel(left_wall, right_wall, depth_normal, options)? {
        Ok(fill) => fill,
        Err(refusal) => return Err(GrooveRefusal::Fill { why: refusal.why, fit: refusal.fit }),
    };
    let body = extrude_planar_regions(&frame, std::slice::from_ref(&fill.region), along[0], along[1])?;
    Ok(GrooveWeld { body, frame, fill, zone })
}

/// A joint (or a stretch of one) with no groove modelled ([`GrooveRefusal::NoGroove`]).
#[derive(Debug, Clone, PartialEq)]
pub struct NoGroove {
    /// The joint's frame: the seams lie in its plane.
    pub frame: Plane,
    /// Where the joint (or the stretch) starts and ends along the frame's normal, from its
    /// origin (mm).
    pub along: [f64; 2],
    /// The seam on each welded side: the arrow side (the outer faces' side), then the other.
    pub seams: [Seam; 2],
}

/// Where the seam runs on one welded side of a joint with no groove: along the joint through
/// `at`, from [`NoGroove::along`]'s start to its end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seam {
    /// The seam's point in the frame's plane (along its `x` and `y`).
    pub at: Vec2,
    /// The outward normal of that side's surface in the frame's plane: the frame's `y`, or its
    /// opposite on the other side.
    pub normal: Vec2,
    /// The plate's thickness there, from this side's surface to the other's (mm).
    pub thickness: f64,
}

/// The mark's default height above the surface (mm, Q98).
pub const MARK_HEIGHT: f64 = 0.2;

impl Seam {
    /// The mark's default width (Q98): the plate's thickness, but 5 mm on plates thicker than
    /// 10 mm.
    #[must_use]
    pub fn mark_width(&self) -> f64 {
        if self.thickness > 10.0 { 5.0 } else { self.thickness }
    }
}

impl NoGroove {
    /// The flat mark on `seam`: a strip `width` wide, centred on the seam, standing `height`
    /// off the surface, along the whole joint (or stretch). Defaults: [`Seam::mark_width`],
    /// [`MARK_HEIGHT`].
    ///
    /// # Errors
    /// [`GrooveRefusal::Invalid`] for a width or height that is not finite and positive; the
    /// errors of the kernel call.
    pub fn mark(&self, seam: &Seam, width: f64, height: f64) -> Result<Body> {
        if !(width.is_finite() && width > 0.0 && height.is_finite() && height > 0.0) {
            return Err(GrooveRefusal::Invalid("a mark's width and height must be finite and positive"));
        }
        // the seam's normal is the frame's y or its opposite: the strip is square to the frame
        let (x, y) = (seam.at.x, seam.at.y);
        let (lo, hi) = if seam.normal.y > 0.0 { (y, y + height) } else { (y - height, y) };
        let corners =
            [Vec2::new(x - width / 2.0, lo), Vec2::new(x + width / 2.0, lo), Vec2::new(x + width / 2.0, hi), Vec2::new(x - width / 2.0, hi)];
        let region = Region2 { outer: Loop2::polygon(&corners), holes: Vec::new() };
        Ok(extrude_planar_regions(&self.frame, &[region], self.along[0], self.along[1])?)
    }
}

/// How far apart two walls are where they are the same number of lines and arcs of the same
/// kinds, in order: the largest difference along `x` or `y` between their ends (and between
/// arcs' centres and radii); `None` for walls of other shapes. shortcut: ends, not the walls'
/// full distance, and only walls read with the same pieces; enough to tell walls that coincide
/// or nearly do.
fn apart(left: &[Seg2], right: &[Seg2]) -> Option<f64> {
    if left.len() != right.len() {
        return None;
    }
    let off = |p: Vec2, q: Vec2| (p.x - q.x).abs().max((p.y - q.y).abs());
    let mut gap: f64 = 0.0;
    for (a, b) in left.iter().zip(right) {
        let ((a0, a1), (b0, b1)) = (ends(a), ends(b));
        gap = gap.max(off(a0, b0)).max(off(a1, b1));
        match (a, b) {
            (Seg2::Line { .. }, Seg2::Line { .. }) => {}
            (Seg2::Arc { center: c, radius: r, .. }, Seg2::Arc { center: k, radius: q, .. }) => gap = gap.max(off(*c, *k)).max((r - q).abs()),
            _ => return None,
        }
    }
    Some(gap)
}

/// The largest coordinate of the walls' ends: the section's size, as the fill takes it for its
/// allowance.
fn size<'s>(walls: impl Iterator<Item = &'s Seg2>) -> f64 {
    walls
        .flat_map(|s| {
            let (a, b) = ends(s);
            [a.x.abs(), a.y.abs(), b.x.abs(), b.y.abs()]
        })
        .fold(0.0, f64::max)
}

/// The first edge a groove face of `pick` (in the order of `faces`) shares with its outer face.
fn toe_edge(pick: &GroovePick<'_>, faces: &[usize]) -> Result<Option<usize>> {
    let outer = pick.part.rim_edges(&[pick.outer])?;
    for &f in faces {
        if let Some(&e) = pick.part.rim_edges(&[f])?.iter().find(|e| outer.contains(e)) {
            return Ok(Some(e));
        }
    }
    Ok(None)
}

fn point(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// The middle of edge `edge` of `part` ([`solidcraft_kernel::edge_middle`]).
fn edge_middle(part: &Body, edge: usize) -> Result<[f64; 3]> {
    match solidcraft_kernel::edge_middle(part, edge)? {
        Some(p) => Ok(point(p)),
        None => Err(GrooveRefusal::Invalid("an edge whose middle the kernel cannot find")),
    }
}

/// The kernel's zone tolerance ([`profiles::zone_tolerance`]).
fn zone_tolerance() -> Result<f64> {
    profiles::zone_tolerance().map_err(|e| GrooveRefusal::Kernel(KernelError::Failed(format!("zone tolerance: {e}"))))
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
    /// surface all along the weld: on the weld face an incompletely filled groove that ISO 5817
    /// permits at no quality level, on the root side root concavity, see [`weld_note`]).
    pub warning: bool,
}

impl GrooveWeld {
    /// The fill's notes in weld words ([`FillNote`] says them in the kernel's).
    #[must_use]
    pub fn notes(&self) -> Vec<WeldNote> {
        self.fill.notes.iter().map(|note| weld_note(note, self.zone)).collect()
    }
}

/// A fill face in weld words.
fn face_name(side: FillSide) -> &'static str {
    match side {
        FillSide::Face => "weld face",
        FillSide::Second => "root-side weld face",
    }
}

/// One fill note in weld words. A concave face below a plate's surface by more than the zone
/// tolerance `zone` (within it, a rounding of the surface's level) is a warning: it dips all along
/// the weld, so it is an incompletely filled groove (ISO 6520-1 no. 511) as a long imperfection,
/// which ISO 5817:2023 permits at no quality level: Table 1, no. 1.14 allows it only as short
/// imperfections (subclauses 3.2 and 3.3: at most 25 mm in any 100 mm of weld, or a quarter of a
/// weld shorter than that), at every level for plates over 3 mm and at D and C only for 0.5 to 3 mm.
/// On the root side the same dip is root concavity (ISO 6520-1 no. 515), which ISO 5817 limits by
/// plate thickness and quality level, all along a weld at some of them: still a warning, but not
/// called 511 and not said to fail every level (CodeRabbit on #22033).
fn weld_note(note: &FillNote, zone: f64) -> WeldNote {
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
        FillNote::BelowToes { side: FillSide::Second, depth, below: [left, right], .. } if left > zone || right > zone => (
            format!(
                "the concave {} dips {depth:.2} mm below the chord, {left:.2} mm below the left plate's root-side surface and \
                 {right:.2} mm below the right one's, all along the weld: root concavity (ISO 6520-1 no. 515); check its depth \
                 against ISO 5817's limit for the plate thickness and quality level",
                face_name(FillSide::Second)
            ),
            true,
        ),
        FillNote::BelowToes { side, depth, below: [left, right], .. } if left > zone || right > zone => (
            format!(
                "the concave {} dips {depth:.2} mm below the chord, {left:.2} mm below the left plate's surface and {right:.2} mm \
                 below the right one's, all along the weld: an incompletely filled groove (ISO 6520-1 no. 511) that ISO 5817 \
                 permits at no quality level (Table 1, no. 1.14 allows it only as short imperfections)",
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

/// How far apart, in zone tolerances, two reads of one point of a section may lie and be taken for
/// one point ([`wall_of`], [`face_read`], and stretches that join in [`cut`]). Each read (a vertex,
/// a toe edge's middle, an end of a line or arc read from an edge, exact but for rounding,
/// [`planar_edges`]) lies on two faces that each run straight along the joint within the zone
/// ([`Body::face_straying`]). A point within the zone of two planes lies within zone / sin(θ/2)
/// of the line where they meet, θ the angle between them, or zone / cos(θ/2) past 90°: under 3.9
/// zones for faces that meet at 30° to 150° (a 60° V's bevel meets its root face at 30°). Two
/// reads of the point are then under 7.8 zones apart, 8 with their rounding. shortcut: faces that
/// meet at under 30° or over 150° (a groove face nearly in line with its neighbour), or that meet
/// tangentially (a round groove's wall at its root, a J's arc at its bevel), may read further
/// apart and are refused as a section that changes; a bound from their angle or the edges' own
/// tolerance if such a groove is refused. The round groove's build test (brett19's point C on
/// #22033 review 6) shows a swept groove's tangent reads within it: its vertices are exact.
const ALLOWANCE: f64 = 8.0;

/// A groove face read across the joint ([`face_read`]).
struct FaceRead {
    /// The record face.
    face: usize,
    /// Where its whole section is present along the joint (mm along `d` from the frame's origin).
    span: [f64; 2],
    /// Its section in the frame, where it reads as one line or arc.
    section: Option<Seg2>,
}

/// Groove face `face` of `pick` read across the joint from its own edges (their vertices in the
/// part's record; brett19's point 1 on #22033 review 5). Its long edges, those whose two
/// ends lie at one point of `frame` (within [`ALLOWANCE`]) and apart along `d`, run along the
/// joint; edges on one such line are one run. Its span is from the last of the runs' starts to the
/// first of their ends: where its whole section is present, so an end not square to the joint is
/// cut where the face is whole. For a plane with straight end edges (a plate's bevel or wall) that
/// is exact; a J or U face's end cut at an angle may reach past its runs' ends inside the arc
/// (shortcut: read the end edge's own extent along `d` when such ends come up).
///
/// Its section: the one line or arc its other edges (its ends) read across the joint
/// ([`Body::planar_edges`]: its ends square to the joint), all reads the same within their
/// tolerance; else (no end square, or an end read in pieces), for a plane, the line between its
/// two runs. `None` for a face with no long edge.
///
/// # Errors
/// [`GrooveRefusal::GrooveFaceInterrupted`] for a face with a hole or cut-out inside it (more than
/// one loop) or a run with a gap along the joint (a notch in its edge): the span from its runs
/// would sweep its section across the gap (CodeRabbit on #22033). The errors of the kernel calls.
fn face_read(pick: &GroovePick<'_>, part: JointPart, face: usize, frame: &Plane, d: [f64; 3], zone: f64) -> Result<Option<FaceRead>> {
    let tol = zone * ALLOWANCE;
    let o = point(frame.origin);
    let edges = pick.part.rim_edges(&[face])?;
    let record = pick.part.record();
    if record.faces.get(face).is_some_and(|f| f.loops.len() > 1) {
        return Err(GrooveRefusal::GrooveFaceInterrupted { part, face });
    }
    // each run along the joint: its point in the frame and how far each of its edges reaches along d
    let mut runs: Vec<(Vec2, Vec<[f64; 2]>)> = Vec::new();
    // the edges across the joint (its ends), read for its section below
    let mut ends: Vec<usize> = Vec::new();
    for &e in &edges {
        let Some(&[a, b]) = record.edges.get(e).map(|edge| &edge.vertices) else { continue };
        let (Some(p), Some(q)) = (record.vertices.get(a as usize), record.vertices.get(b as usize)) else { continue };
        let (p, q) = (p.point, q.point);
        let at = |p: [f64; 3]| Vec2::new(affine::distance_along(o, point(frame.x), p), affine::distance_along(o, point(frame.y), p));
        let (s, t) = (affine::distance_along(o, d, p), affine::distance_along(o, d, q));
        if !near(at(p), at(q), tol) || (t - s).abs() <= tol {
            ends.push(e);
            continue;
        }
        let reach = [s.min(t), s.max(t)];
        match runs.iter_mut().find(|(x, _)| near(*x, at(p), tol)) {
            Some((_, r)) => r.push(reach),
            None => runs.push((at(p), vec![reach])),
        }
    }
    if runs.is_empty() {
        return Ok(None);
    }
    // each run's edges end to end; a gap between them is a notch in the face's edge
    let mut whole: Vec<(Vec2, [f64; 2])> = Vec::new();
    for (x, reaches) in &mut runs {
        reaches.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let Some((&first, rest)) = reaches.split_first() else { continue };
        let mut run = first;
        for r in rest {
            if r[0] > run[1] + tol {
                return Err(GrooveRefusal::GrooveFaceInterrupted { part, face });
            }
            run[1] = run[1].max(r[1]);
        }
        whole.push((*x, run));
    }
    let span = whole.iter().fold([f64::NEG_INFINITY, f64::INFINITY], |[lo, hi], &(_, [a, b])| [lo.max(a), hi.min(b)]);
    let agree = pick.part.planar_edges_tolerance().max(zone);
    let segs: Vec<Seg2> = pick.part.planar_edges(frame, &ends).into_iter().flatten().filter_map(|p| one_seg(&p)).collect();
    let from_ends = segs.split_first().and_then(|(a, rest)| rest.iter().all(|b| same(a, b, agree)).then_some(*a));
    let section = match (from_ends, whole.as_slice()) {
        (Some(s), _) => Some(s),
        (None, [(a, _), (b, _)]) if pick.part.face_plane_normal(face)?.is_some() => Some(Seg2::Line { a: *a, b: *b }),
        _ => None,
    };
    Ok(Some(FaceRead { face, span, section }))
}

/// A part's wall in the joint's frame, from its toe `toe` (in the frame) away from the face: each
/// groove face's section ([`face_read`], from `reads`), each starting where the one before ends.
/// `None` where a face has no section or does not start there.
fn wall_of(reads: &[FaceRead], chain: &Chain, toe: Vec2, zone: f64) -> Option<Vec<Seg2>> {
    let tol = zone * ALLOWANCE;
    let mut wall: Vec<Seg2> = Vec::new();
    let mut at = toe;
    for face in &chain.faces {
        let seg = reads.iter().find(|r| r.face == *face)?.section?;
        let (a, b) = ends(&seg);
        let seg = if near(a, at, tol) {
            seg
        } else if near(b, at, tol) {
            reversed(&seg)
        } else {
            return None;
        };
        at = ends(&seg).1;
        wall.push(seg);
    }
    Some(wall)
}

/// The piece of one edge's read when it is a single line or arc.
fn one_seg(pieces: &[PlanarPiece]) -> Option<Seg2> {
    match pieces {
        [PlanarPiece::Seg(s @ (Seg2::Line { .. } | Seg2::Arc { .. }))] => Some(*s),
        _ => None,
    }
}

/// A line's or an arc's two ends. shortcut: an arc's ends by its angles in plain trigonometry,
/// as the app's sheet-metal code reads them; a kernel call for it if reviewers ask.
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
        (Seg2::Arc { center: c, radius: r, start: s0, sweep: w0 }, Seg2::Arc { center: k, radius: q, start: s1, sweep: w1 }) => {
            near(*c, *k, tol) && (r - q).abs() <= tol && same_turn((*s0, *w0), (*s1, *w1), tol / r.max(tol))
        }
        _ => false,
    };
    ends_agree && kind_agrees
}

/// Whether two arcs of one circle turn over the same angles, either way round: each one's start
/// and sweep taken counter-clockwise, compared within `eps` radians. Two halves of one circle have
/// the same ends, centre and radius, but not the same start (CodeRabbit on #22033).
fn same_turn((s0, w0): (f64, f64), (s1, w1): (f64, f64), eps: f64) -> bool {
    let ccw = |s: f64, w: f64| if w < 0.0 { (s + w, -w) } else { (s, w) };
    let ((a, u), (b, v)) = (ccw(s0, w0), ccw(s1, w1));
    let off = (a - b).rem_euclid(std::f64::consts::TAU);
    (u - v).abs() <= eps && (off <= eps || off >= std::f64::consts::TAU - eps)
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
