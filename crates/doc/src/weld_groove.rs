//! Groove welds (#18500): the joint of a groove (butt) weld, read from the two parts the user
//! picked. This first piece reads each part's groove faces as one chain from the toe ([`chain`]);
//! the joint's direction and frame, its stretches and the weld body follow on top of it.
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
//! | [`GrooveRefusal`] | Why a joint is refused                                              |
//!
//! # Not covered
//!
//! T joints, whose toe on the through part is no edge, are refused ([`GrooveRefusal::TJoint`]);
//! their toe and the seam mark come in a later PR (brett19 on #18500).

use solidcore_ops::{Body, Order};

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
    /// An ops call failed.
    #[error(transparent)]
    Ops(#[from] solidcore_ops::Error),
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

/// A part's groove faces in order from the toe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    /// The groove faces (record indices), the one beside the outer face first, each next one
    /// sharing an edge with the one before.
    pub faces: Vec<usize>,
    /// The toe: the edges (record indices) the first face shares with the outer face.
    pub toe: Vec<usize>,
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
    // each face's edges, the outer face's last
    let edges = faces.iter().chain([&pick.outer]).map(|&f| pick.part.rim_edges(pick.order, &[f])).collect::<solidcore_ops::Result<Vec<_>>>()?;
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

#[cfg(test)]
#[path = "weld_groove_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
