//! Finding a face or edge again by the name a feature stored for it.
//!
//! The face with that name is the face (a face its feature made from several of its roles, a
//! side drawn in collinear pieces, has each as a name); else the one face answering to it as
//! exactly: the named piece told apart by the faces around it, which still bound it. Nothing
//! else is: when the named face was split into several pieces, when it was merged into another
//! face, when only a face of the same stem is left, when the name is on several bodies, or when
//! the name was saved before format 14 ([`LEGACY_MARK`]), the name finds nothing and the feature
//! that uses it fails, asking for the reference to be picked again
//! ([`crate::DocError::Reselect`]), or, for a sketch's projection, the sketch fails naming the
//! projections it lost ([`crate::DocError::LostProjections`]). An edge name has two more ways:
//! when no edge's faces answer to it, and one of its faces is gone from a feature still on the
//! body, the reference's point decides (see [`renamed_edge`]); so it does when a face of it is
//! named after neighbours it no longer has, or answers to several faces, and the point lies on
//! one edge between them (see [`resuffixed_edge`]). An extrude's From or To face stands only for
//! where it lies, so a split one is the piece the reference's point lies on (see
//! [`locate_level_face_on_body`], #22281). A face whose name was worked out from
//! geometry, found where the reference's point no longer lies, is reported: only geometry decided.
//! Saved numbered Project links also have a compatibility check for a whole face that
//! contains the saved pick and comes directly from its original construction feature.
//! This check is local to manual sketch projection. A thread alone also finds a round side
//! again that 0.30.1 saved as a numbered piece (`F2:side#2`) or under a later index
//! (`F4:bore:side:1`): see [`locate_thread_face_on_body`] (#16602).

use std::sync::Arc;

use solidcore_record::Vec3;
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;

use crate::history_names::{Match, Names};
use crate::naming::{self, closest, face_names, strip_piece};
use crate::piece_names::strip_edge_number;
use crate::{FeatureKind, ModelBody, ModelState, Operation};

/// The mark on a name saved before format 14 (`?F2:top#2`). Those names were worked out from
/// geometry, and some of them name a face that exact history names differently (a tube's outer
/// wall was named after the nearest sketch curve to its centre, its inner circle): such a name
/// finds no face, and the feature using it asks for the face to be picked again, which stores
/// its exact name (a sketch names the projection it lost; repairing it stores the exact name).
pub const LEGACY_MARK: char = '?';

/// A name saved before format 14, without its mark (`None`: an exact name).
pub fn legacy_name(name: &str) -> Option<&str> {
    name.strip_prefix(LEGACY_MARK)
}

/// Where a stored edge name was found.
#[derive(Clone, Copy, Debug)]
pub struct EdgeHit {
    /// The edge (its index in the body's mesh).
    pub edge: usize,
    /// A point on the edge: the reference's own point when it lies on it, else the edge's middle.
    pub point: Vec3,
    /// Whether the names of the faces it separates follow exact history (and the edge is not
    /// one of several between them, numbered by where they lie).
    pub verified: bool,
    /// Whether the reference's point still lies on the edge.
    pub on_point: bool,
}

/// A body's edge names and face names, worked out once for every edge name looked up on it.
pub(crate) struct BodyEdges {
    pub(crate) mesh: Arc<Mesh>,
    edges: Arc<naming::EdgeNameIndex>,
    faces: Arc<Names>,
}

/// The edges of a body a stored edge name matches, before a picked point decides between them.
pub(crate) enum EdgeMatch {
    /// The edge with that name, or the one edge between the faces it names.
    Edge(usize),
    /// Pieces of it: the faces it names meet along several edges now, or only a piece of one of
    /// them is left.
    Pieces(Vec<usize>),
}

impl BodyEdges {
    pub(crate) fn new(b: &ModelBody) -> BodyEdges {
        BodyEdges { mesh: b.mesh(), edges: naming::edge_name_index(b), faces: naming::names(b) }
    }

    /// Edge `i`'s persistent name.
    pub(crate) fn name(&self, i: usize) -> Option<&str> {
        self.edges.names.get(i).map(String::as_str)
    }

    /// Whether edge `i`'s name follows exact history: the faces it separates do, and it isn't one
    /// of several edges between them, numbered by where they lie.
    pub(crate) fn verified(&self, i: usize) -> bool {
        self.mesh.edge_faces.get(i).is_some_and(|fs| fs.iter().all(|f| self.faces.verified(*f as usize)))
            && self.edges.names.get(i).is_some_and(|n| strip_edge_number(n).len() == n.len())
    }

    /// What the stored edge name `name` matches: the edge with that name; else the one edge
    /// between the faces it names (followed through splits and merges of those faces); else the
    /// pieces of it. `None` when nothing does.
    pub(crate) fn matching(&self, name: &str) -> Option<EdgeMatch> {
        if let Some(&i) = self.edges.by_name.get(name) {
            return Some(EdgeMatch::Edge(i));
        }
        // By the faces it separates: the best way any edge matches.
        let wanted = strip_edge_number(name);
        let mut best: Option<(Match, Vec<usize>)> = None;
        for (i, fs) in self.mesh.edge_faces.iter().enumerate() {
            let [fa, fb] = fs.as_slice() else { continue };
            let Some(level) = edge_match(&self.faces, *fa as usize, *fb as usize, wanted) else { continue };
            keep_best(&mut best, level, i);
        }
        let (level, cands) = best?;
        let one = match cands.as_slice() {
            [i] if level >= Match::Piece => Some(*i),
            _ => None,
        };
        Some(one.map_or(EdgeMatch::Pieces(cands), EdgeMatch::Edge))
    }
}

/// The edge of a body a stored edge name finds: the edge with that name; else the one edge
/// between the faces it names exactly (see [`exactly`]). A numbered name (`~k`) was one of
/// several edges between its faces, told apart only by where they lay, so the one edge left
/// between them is it only where the reference's point still lies on it. A name whose faces no
/// edge's faces answer to at all, one of them gone from a feature that is still there, is the edge
/// of the other face the reference's point lies on, clear of its ends (see [`renamed_edge`]).
/// A name with a face named after neighbours it no longer has, or answering to several faces, is
/// the one edge between faces answering to it that the reference's point lies on (see
/// [`resuffixed_edge`]).
/// `Err` explains when there is none: no such edge, the named edge split into several, one of
/// several edges whose faces now bound one edge elsewhere, or a name saved before format 14.
pub fn locate_edge(b: &ModelBody, name: &str, near: Vec3) -> Result<EdgeHit, String> {
    if legacy_name(name).is_some() {
        return Err(format!("edge `{name}` was named by a build before format 14"));
    }
    let m = b.mesh();
    let edges = naming::edge_name_index(b);
    let faces = naming::names(b);
    let mid = |i: usize| edge_mid(&m, i);
    // Several edges between the same two faces are numbered by where they lie: not exact.
    let verified = |i: usize| {
        m.edge_faces.get(i).is_some_and(|fs| fs.iter().all(|f| faces.verified(*f as usize)))
            && edges.names.get(i).is_some_and(|n| strip_edge_number(n).len() == n.len())
    };
    let exact = |i: usize| -> Result<EdgeHit, String> {
        let on = edge_near(b, &m, i, near);
        mid(i).map(|p| EdgeHit { edge: i, point: if on { near } else { p }, verified: verified(i), on_point: on }).ok_or_else(|| "edge".into())
    };
    if let Some(i) = edges.find_exact(name) {
        return exact(i);
    }
    // By the faces it separates: the best way any edge matches.
    let wanted = strip_edge_number(name);
    let (mut best, mut loose): (Option<(Match, Vec<usize>)>, bool) = (None, false);
    for (i, fs) in m.edge_faces.iter().enumerate() {
        let [fa, fb] = fs.as_slice() else { continue };
        let Some(level) = edge_match(&faces, *fa as usize, *fb as usize, wanted) else { continue };
        if exactly(level) {
            keep_best(&mut best, level, i);
        } else {
            loose = true;
        }
    }
    let at_pick = |i: usize| EdgeHit { edge: i, point: near, verified: false, on_point: true };
    let Some((_, cands)) = best else {
        let picked = if loose { resuffixed_edge(b, &m, &faces, wanted, near) } else { renamed_edge(b, &m, &faces, wanted, near) };
        return picked.map(at_pick).ok_or_else(|| format!("edge `{name}` no longer exists"));
    };
    // When the faces of several numbered edges bound one edge now, every one of their names
    // finds the faces of that edge: taking it for each would put them all on one edge (#13332).
    let numbered = wanted.len() != name.len();
    match cands.as_slice() {
        [i] if numbered && !edge_near(b, &m, *i, near) => {
            Err(format!("edge `{name}` was one of several between its faces, and the one left is elsewhere"))
        }
        [i] => exact(*i),
        _ => resuffixed_edge(b, &m, &faces, wanted, near).map(at_pick).ok_or_else(|| format!("edge `{name}` is no longer one edge")),
    }
}

/// Edge name `name` without the numbers that tell its faces apart from tied pieces of the same
/// name (`F2:side:l1|F2:top#1` is `F2:side:l1|F2:top`): `None` when none of its faces has one, or
/// it does not read as an edge name.
fn without_face_ties(name: &str) -> Option<crate::persistent_name::EdgeName> {
    use crate::persistent_name::{EdgeName, Suffix};
    let mut e = EdgeName::parse(name).ok()?;
    let mut tied = false;
    for f in &mut e.faces {
        match f.suffixes.last_mut() {
            Some(Suffix::Numbered(_)) => {
                f.suffixes.pop();
                tied = true;
            }
            Some(Suffix::Piece(q)) if q.tie.is_some() => {
                q.tie = None;
                tied = true;
            }
            _ => {}
        }
    }
    tied.then_some(e)
}

/// The edge a blend's stored edge name `name` meant when it found edge `found` of body `b` (whose
/// mesh is `m`) and the blend's own pick `near` does not lie on it: the edge `near` lies on
/// (exactly, clear of its ends, see [`edge_through`]), when its name differs from `name` only in
/// the numbers of its faces. Pieces that nothing else tells apart are numbered along where they
/// lie, and builds have numbered them differently: both caps of an extrude with a start offset are
/// `top`, and since #21762 the one a design saved by 0.47.0 called `top#1` is `top#2` (#22230).
/// The point was picked on the edge the name was given to, so it says which one that is, but only
/// on the body it was picked on: a pick is not updated when an edit moves the body, and then it
/// can lie on the other cap's edge. The caller checks that the body has not moved: its box is the
/// saved one, and no earlier feature places bodies whole.
pub(crate) fn retied_edge(b: &ModelBody, m: &Mesh, name: &str, found: usize, near: Vec3) -> Option<usize> {
    if b.body.is_mesh() || !near.is_finite() || edge_near(b, m, found, near) {
        return None;
    }
    let wanted = without_face_ties(name)?;
    let j = edge_through(b, m, near).filter(|j| *j != found)?;
    (without_face_ties(naming::edge_name_index(b).names.get(j)?)? == wanted).then_some(j)
}

/// Find a saved edge in its body when it may be named with numbered pieces: a Sketch Project edge, or
/// the edge of a fillet or chamfer (#16535). Older designs can name the edge between
/// numbered pieces of faces that now evaluate whole (`F4:side:l6#2|F4:side:l7#2`, now
/// `F4:side:l6|F4:side:l7`). Recover it only when the faces it separates are whole, the saved
/// point lies on it, and its name is exact. A name that ends with the number of the edge a blend
/// replaced is found at its point too (see [`recover_blend_numbered_edge`]), as are the unreliable
/// ordinals once assigned by Align (see [`recover_align_projected_edge`]). Other edge references
/// still require the stored name as picked.
pub(crate) fn locate_projected_edge(b: &ModelBody, name: &str, near: Vec3) -> Result<EdgeHit, String> {
    locate_projected_edge_from(b, &b.name, name, near)
}

/// As [`locate_projected_edge`], retaining the saved source body's name when Project searches
/// surviving bodies after a join. Align ordinals alone never identify which aligned body was picked.
pub(crate) fn locate_projected_edge_from(b: &ModelBody, saved_body: &str, name: &str, near: Vec3) -> Result<EdgeHit, String> {
    locate_edge(b, name, near).or_else(|miss| {
        recover_numbered_projected_edge(b, name, near)
            .or_else(|| recover_blend_numbered_edge(b, name, near))
            .or_else(|| recover_align_projected_edge(b, saved_body, name, near))
            .or_else(|| recover_misnumbered_side_edge(b, name, near))
            .or_else(|| recover_positional_edge(b, name, near))
            .ok_or(miss)
    })
}

/// The number of face `name` names on body `b` by position (`<body>:face:<k>`): a body past the
/// naming depth limit names its faces so (see `naming::names`). `None` for any other name.
fn positional_face(b: &ModelBody, name: &str) -> Option<usize> {
    let k = name.strip_prefix(b.name.as_str())?.strip_prefix(":face:")?;
    if k.is_empty() || !k.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    k.parse().ok().filter(|k| *k < b.body.face_count())
}

/// An edge saved by position (`Cleat:face:0|Cleat:face:2`) on a body that is named from its
/// history now (#20956). The naming depth counts every feature before, so deleting earlier
/// features, even ones that never touched the body, can bring a body named past the limit back
/// under it. The numbers are the faces' places in the kernel's order, which is all such a name
/// ever said: the one exact edge between those two faces that the saved point lies on.
fn recover_positional_edge(b: &ModelBody, name: &str, near: Vec3) -> Option<EdgeHit> {
    if b.body.is_mesh() || !near.is_finite() {
        return None;
    }
    let parts = split_top_level_bars(strip_edge_number(name));
    let [fa, fb] = parts.as_slice() else { return None };
    let (fa, fb) = (positional_face(b, fa)?, positional_face(b, fb)?);
    let reach = pick_reach(b);
    let mut on = b.body.record().edges.iter().enumerate().filter(|(i, e)| {
        let [p, q] = e.uses.as_slice() else { return false };
        let (p, q) = (p.face as usize, q.face as usize);
        ((p, q) == (fa, fb) || (q, p) == (fa, fb)) && solidcraft_kernel::edge_distance(&b.body, *i, near).ok().flatten().is_some_and(|d| d <= reach)
    });
    let edge = on.next().filter(|_| on.next().is_none())?.0;
    Some(EdgeHit { edge, point: near, verified: false, on_point: true })
}

/// A face saved by position (`Cleat:face:3`) on a body named from its history now: face 3 of
/// that body, when the saved point lies on it (see [`recover_positional_edge`]).
fn recover_positional_face(st: &ModelState, name: &str, p: Vec3) -> Option<FaceHit> {
    if !p.is_finite() {
        return None;
    }
    let (body, b, face) =
        st.bodies.iter().enumerate().filter(|(_, b)| !b.body.is_mesh()).find_map(|(i, b)| Some((i, b, positional_face(b, name)?)))?;
    let (_, point) = on_face(b, face, p)?;
    Some(FaceHit { body, face, point, verified: false, on_point: true })
}

/// Align used to discard its transform's history and give faces it could not find at the
/// unmoved position names such as `F3:face:0`. rc.4 and 0.45.0 could enumerate those faces
/// differently (#20288). Neither numbering is a persistent identity: recover a saved Project
/// link only on its saved body or its descendant through proven non-copy rigid transforms and
/// joins, at one unique exact edge through its saved point. No mesh or nearest-edge substitution
/// decides this, and another body moved by the same Align gets no fallback.
fn recover_align_projected_edge(b: &ModelBody, saved_body: &str, name: &str, near: Vec3) -> Option<EdgeHit> {
    use crate::persistent_name::{Base, EdgeName, Role};
    if b.body.is_mesh() || !near.is_finite() {
        return None;
    }
    let read = EdgeName::parse(name).ok()?;
    if read.tie.is_some()
        || read.faces.len() != 2
        || read.faces.first() == read.faces.get(1)
        || !read.faces.iter().any(|face| matches!(&face.base, Base::Made { role: Role::Index(_), .. }))
    {
        return None;
    }
    let record = b.body.record();
    let mut on = record.edges.iter().enumerate().filter(|(i, edge)| {
        let tolerance = record.tolerance.max(edge.tolerance);
        tolerance.is_finite() && solidcraft_kernel::edge_distance(&b.body, *i, near).ok().flatten().is_some_and(|d| d <= tolerance)
    });
    let edge = on.next().filter(|_| on.next().is_none())?.0;
    let found = record.edges.get(edge)?;
    if found.uses.len() != 2 {
        return None;
    }
    let mut ancestor = b;
    let mut ancestor_edge = edge;
    let mut from_saved_body = b.name == saved_body;
    let mut align = None;
    // Bound the walk even for an unusually long or damaged naming chain.
    for _ in 0..400 {
        let origin = ancestor.names.origin.as_ref()?;
        if let FeatureKind::Align { bodies, .. } = &origin.feature.kind
            && bodies.iter().any(|body| *body == ancestor.name)
            && read.faces.iter().any(|face| matches!(&face.base, Base::Made { feature, role: Role::Index(_) } if feature.0 == origin.feature.id))
        {
            if !from_saved_body {
                return None;
            }
            align = Some((origin.feature.id, ancestor.body.face_count()));
            break;
        }
        (ancestor, ancestor_edge) = naming::inherited_edge_predecessor(ancestor, ancestor_edge)?;
        from_saved_body |= ancestor.name == saved_body;
    }
    let (align, face_count) = align?;
    let mut numbered = false;
    for face in &read.faces {
        if let Base::Made { feature, role: Role::Index(index) } = &face.base
            && feature.0 == align
        {
            if !face.suffixes.is_empty() || (*index as usize) >= face_count {
                return None;
            }
            numbered = true;
        }
    }
    if !numbered {
        return None;
    }
    let faces = naming::names(b);
    // An ordinary face name, where Align happened to retain one by overlap, must still
    // name an incident face. Only this Align's unreliable ordinals are read at the point.
    let incident = |edge: &solidcore_record::brep::Edge| {
        read.faces.iter().all(|face| match &face.base {
            Base::Made { feature, role: Role::Index(_) } if feature.0 == align => true,
            _ => edge.uses.iter().any(|u| crate::history_names::matches(&faces, u.face as usize, &face.to_string()).is_some_and(exactly)),
        })
    };
    if !incident(found) {
        return None;
    }
    Some(EdgeHit { edge, point: near, verified: false, on_point: true })
}

/// 0.45.0-rc.4 and 0.45.0 could name several faces a feature swept from different curves after
/// one of them, told apart by a number only: a revolve cut after another named both flanks of its
/// groove `F6:side:l1#1` and `F6:side:l1#2`, where earlier and later builds name them
/// `F6:side:l1` and `F6:side:l2` (#21293). A saved edge naming such a numbered side face is found
/// at its saved point: the one exact edge within a pick's reach of it, where the numbered face
/// is a side face the same feature swept from the same profile and the other face is the saved
/// face or a piece of it. No mesh or nearest-edge substitution decides this.
fn recover_misnumbered_side_edge(b: &ModelBody, name: &str, near: Vec3) -> Option<EdgeHit> {
    use crate::persistent_name::{Base, EdgeName, FaceName, Role, Suffix};
    if b.body.is_mesh() || !near.is_finite() {
        return None;
    }
    let read = EdgeName::parse(name).ok()?;
    let side = |f: &FaceName| match &f.base {
        Base::Made { feature, role: Role::Side { profile, .. } } => Some((*feature, *profile)),
        _ => None,
    };
    let numbered = |f: &FaceName| side(f).is_some() && matches!(f.suffixes.as_slice(), [Suffix::Numbered(_)]);
    let [first, second] = read.faces.as_slice() else { return None };
    // One face numbered: the other, named as saved, ties the edge down. A name whose faces are
    // both numbered pieces (as 0.30.1 saved a revolve's) says nothing that tells the edge apart.
    // The other face carries no piece number at all: one numbered side face is the only
    // misnumbering this reads past.
    let carries_number = |f: &FaceName| f.suffixes.iter().any(|s| matches!(s, Suffix::Numbered(_)));
    if numbered(first) == numbered(second) || (carries_number(first) && carries_number(second)) {
        return None;
    }
    // The numbered face is a cut's flank and the other the face it cut, made earlier: a name
    // whose faces one feature made (a numbered piece of a side beside its own top) is a split
    // face's piece, which the saved number alone can't pick out.
    let (flank, other) = if numbered(first) { (first, second) } else { (second, first) };
    if other.made_by().is_none_or(|other| Some(other) == flank.made_by()) {
        return None;
    }
    let record = b.body.record();
    let reach = pick_reach(b);
    let mut on = record
        .edges
        .iter()
        .enumerate()
        .filter(|(i, _)| solidcraft_kernel::edge_distance(&b.body, *i, near).ok().flatten().is_some_and(|d| d <= reach));
    let edge = on.next().filter(|_| on.next().is_none())?.0;
    let [p, q] = record.edges.get(edge)?.uses.as_slice() else { return None };
    let faces = naming::names(b);
    let current = |face: u32| faces.faces.get(face as usize).and_then(|n| FaceName::parse(n).ok());
    let (p, q) = (current(p.face)?, current(q.face)?);
    let answers = |saved: &FaceName, now: &FaceName| {
        if numbered(saved) {
            side(saved) == side(now) && now.suffixes.iter().all(|s| matches!(s, Suffix::Piece(_)))
        } else {
            now.stem() == saved.stem()
        }
    };
    ((answers(first, &p) && answers(second, &q)) || (answers(first, &q) && answers(second, &p))).then_some(EdgeHit {
        edge,
        point: near,
        verified: false,
        on_point: true,
    })
}

/// Find a saved edge between a face and a fillet's or chamfer's face whose name, in the older
/// form, ends with the number of the edge the blend replaced: 0.30.1 saved the top edge of a round
/// that replaced the second of two edges between the same faces as
/// `F2:top|F3:blend:F2:side:c1|F2:side:l1~2` (#14479). The older form reads a trailing `~k` as the
/// whole edge's number, so the name finds no edge as read. Read with `~k` on the blend's edge
/// (`F2:top|F3:blend(F2:side:c1|F2:side:l1~2)`), it can still answer to several faces alike: a
/// blend of several edges between the same two faces names each of its faces after all of them,
/// told apart by a number only. So the saved point decides: the one edge between the other face
/// and a face answering to the blend's name that the point lies on.
fn recover_blend_numbered_edge(b: &ModelBody, name: &str, near: Vec3) -> Option<EdgeHit> {
    use crate::persistent_name::{Base, EdgeName, Rounds};
    if b.body.is_mesh() || !near.is_finite() || !crate::persistent_name::in_old_form(name) {
        return None;
    }
    let read = EdgeName::parse(name).ok()?;
    let [face, blend] = read.faces.as_slice() else { return None };
    let (Some(tie), Base::Blend { feature, rounds: Rounds::Edge(edge) }) = (read.tie, &blend.base) else { return None };
    if !blend.suffixes.is_empty() || edge.tie.is_some() {
        return None;
    }
    let rounds = Rounds::Edge(Box::new(EdgeName { faces: edge.faces.clone(), tie: Some(tie) }));
    let blend = crate::persistent_name::FaceName { base: Base::Blend { feature: *feature, rounds }, suffixes: Vec::new() };
    let (face, blend) = (face.to_string(), blend.to_string());
    let faces = naming::names(b);
    // The face exactly (see [`exactly`]); the blend face by its name, a piece of it, or a name
    // merged into it.
    let level = |f: u32, x: &str| crate::history_names::matches(&faces, f as usize, x);
    let between = |p: u32, q: u32| level(p, &face).is_some_and(exactly) && level(q, &blend).is_some_and(|l| l >= Match::Piece);
    let reach = pick_reach(b);
    // Of the B-rep's edges between those faces, the one whose exact curve the point lies on.
    let mut on = b.body.record().edges.iter().enumerate().filter(|(i, e)| {
        let [p, q] = e.uses.as_slice() else { return false };
        (between(p.face, q.face) || between(q.face, p.face))
            && solidcraft_kernel::edge_distance(&b.body, *i, near).ok().flatten().is_some_and(|d| d <= reach)
    });
    let edge = on.next().filter(|_| on.next().is_none())?.0;
    Some(EdgeHit { edge, point: near, verified: false, on_point: true })
}

fn recover_numbered_projected_edge(b: &ModelBody, name: &str, near: Vec3) -> Option<EdgeHit> {
    if b.body.is_mesh() || !near.is_finite() {
        return None;
    }
    // Each face of the name without its piece number (`F4:side:l6#2` is `F4:side:l6`).
    let mut numbered = false;
    let whole: Vec<&str> = name
        .split('|')
        .map(|part| match part.rsplit_once('#') {
            Some((stem, number))
                if !stem.is_empty()
                    && strip_piece(stem) == stem
                    && !number.is_empty()
                    && !number.starts_with('0')
                    && number.bytes().all(|digit| digit.is_ascii_digit()) =>
            {
                numbered = true;
                stem
            }
            _ => part,
        })
        .collect();
    if !numbered {
        return None;
    }
    let hit = locate_edge(b, &whole.join("|"), near).ok().filter(|hit| hit.verified && hit.on_point)?;
    // Each face it separates is whole: no other piece of it is left to be the one picked. The
    // edge is verified by that name, so the faces it separates are the `whole` stems.
    whole.iter().all(|face| faces_with_stem(b, face).len() == 1).then_some(hit)
}

/// The pieces of a saved whole closed edge that a later feature split by putting a vertex on it
/// (#21997: a whole-turn revolve join's turned seam does): the saved name then answers to the
/// numbered edges between its faces (`…~1`, `…~2`). Every such piece is returned when they join
/// end to end, through the vertices they share in the B-rep, into the one closed curve the saved
/// edge was, every edge between the name's faces belongs to that curve, and the reference's
/// point lies on one of them, measured on its exact curve: the pieces together are that edge, as
/// a pick made today stores one name per piece of it. Pieces that do not close again (a later
/// feature took part of the curve), a point on none of them, or another edge between the same
/// faces outside that curve (a second intersection, split or whole), and the name asks to be
/// picked again.
pub(crate) fn recover_split_closed_edge(b: &ModelBody, name: &str, near: Vec3) -> Option<Vec<EdgeHit>> {
    if b.body.is_mesh() || !near.is_finite() {
        return None;
    }
    // A numbered name (`~k`) already names one of several edges: [`resuffixed_edge`] reads those.
    if strip_edge_number(name).len() != name.len() {
        return None;
    }
    let faces = naming::names(b);
    let record = b.body.record();
    // The B-rep's edges between the name's faces, each face answering exactly (see [`exactly`]).
    let mut cands: Vec<usize> = Vec::new();
    for (i, e) in record.edges.iter().enumerate() {
        let [p, q] = e.uses.as_slice() else { continue };
        if edge_match(&faces, p.face as usize, q.face as usize, name).is_some_and(exactly) {
            cands.push(i);
        }
    }
    let reach = pick_reach(b);
    let on_curve = |i: usize| solidcraft_kernel::edge_distance(&b.body, i, near).ok().flatten().is_some_and(|d| d <= reach);
    // The piece the reference's point lies on, on its exact curve.
    let start = cands.iter().copied().find(|&i| on_curve(i))?;
    // The pieces that join with it through the vertices they share in the B-rep.
    let mut pieces = vec![start];
    let mut grew = true;
    while grew {
        grew = false;
        for &i in &cands {
            if pieces.contains(&i) {
                continue;
            }
            let shares = pieces
                .iter()
                .any(|&j| record.edges.get(j).zip(record.edges.get(i)).is_some_and(|(a, e)| a.vertices.iter().any(|v| e.vertices.contains(v))));
            if shares {
                pieces.push(i);
                grew = true;
            }
        }
    }
    // The one closed curve the saved edge was: more than one piece, every vertex of it the end
    // of exactly two of them, and no edge between the same faces outside it: another such curve
    // makes the name ambiguous, and it asks to be picked again.
    if pieces.len() < 2 {
        return None;
    }
    let mut ends: Vec<u32> = pieces.iter().filter_map(|&i| record.edges.get(i)).flat_map(|e| e.vertices).collect();
    ends.sort_unstable();
    if !ends.chunk_by(|a, b| a == b).all(|group| group.len() == 2) {
        return None;
    }
    if cands.iter().any(|i| !pieces.contains(i)) {
        return None;
    }
    // The reference's own point names one piece only: at a vertex two pieces share it, and every
    // piece then takes its own middle, so the blend receives each piece as its own pick.
    let on: Vec<bool> = pieces.iter().map(|&i| on_curve(i)).collect();
    let named_by_pick = on.iter().filter(|on| **on).count() == 1;
    let mut hits = Vec::new();
    for (&i, on_curve_it) in pieces.iter().zip(&on) {
        let on_point = *on_curve_it && named_by_pick;
        let point = if on_point { near } else { solidcraft_kernel::edge_middle(&b.body, i).ok().flatten()? };
        hits.push(EdgeHit { edge: i, point, verified: false, on_point });
    }
    Some(hits)
}

/// Where a stored vertex name was found.
#[derive(Clone, Copy, Debug)]
pub struct VertexHit {
    /// The vertex (its index in the body's B-rep record).
    pub vertex: usize,
    /// The vertex's point in world coordinates.
    pub point: Vec3,
    /// Whether the names of the faces meeting at it follow exact history (and the vertex is not
    /// one of several with the same incident faces, numbered by where they lie).
    pub verified: bool,
    /// Whether the reference's point still lies on the vertex.
    pub on_point: bool,
}

impl VertexHit {
    /// What a feature says about how it found a vertex by its stored name `name` (`None`: exactly).
    pub fn note(&self, name: &str) -> Option<String> {
        if !self.verified && !self.on_point {
            Some(format!("vertex `{}` was found by geometry, not exact history; check it", quoted(name)))
        } else {
            None
        }
    }
}

/// The vertex of a body a stored vertex name finds: the vertex with that name; else the one
/// vertex whose incident faces answer to it cleanly via exact history. A numbered name (`~k`)
/// was one of several vertices with the same incident faces, told apart only by where they lay,
/// so the one vertex left is it only where the reference's point still lies on it.
pub fn locate_vertex(b: &ModelBody, name: &str, near: Vec3) -> Result<VertexHit, String> {
    if legacy_name(name).is_some() {
        return Err(format!("vertex `{name}` was named by a build before format 14"));
    }
    if b.body.is_mesh() {
        return Err(format!("vertex `{name}` cannot be found on a mesh body"));
    }
    let record = b.body.record();
    let vertices = naming::vertex_name_index(b);
    let incident = &vertices.incident;
    let faces = naming::names(b);
    let reach = pick_reach(b);
    let pos = |v: usize| record.vertices.get(v).map(|v| Vec3::new(v.point[0], v.point[1], v.point[2])).filter(|p| p.is_finite());
    let verified = |v: usize| {
        incident.get(v).is_some_and(|fs| !fs.is_empty() && fs.iter().all(|f| faces.verified(*f as usize)))
            && vertices.names.get(v).is_some_and(|n| strip_edge_number(n).len() == n.len())
    };
    let hit = |v: usize| -> Result<VertexHit, String> {
        let point = pos(v).ok_or_else(|| "vertex".to_string())?;
        let on_point = near.is_finite() && point.dist(near) <= reach;
        Ok(VertexHit { vertex: v, point, verified: verified(v), on_point })
    };
    if let Some(v) = vertices.find_exact(name) {
        return hit(v);
    }
    let (wanted, numbered) = parse_vertex_wanted(name);
    let wanted_parsed: Vec<(String, Option<crate::persistent_name::FaceName>)> =
        wanted.iter().map(|w| (w.clone(), crate::persistent_name::FaceName::parse(w).ok())).collect();
    let (mut best, mut loose): (Option<(Match, Vec<usize>)>, bool) = (None, false);
    for (v, fs) in incident.iter().enumerate() {
        if fs.is_empty() {
            continue;
        }
        let Some(level) = vertex_match(&faces, fs, &wanted_parsed) else { continue };
        if exactly(level) {
            keep_best(&mut best, level, v);
        } else {
            loose = true;
        }
    }
    let at_pick = |v: usize| -> Option<VertexHit> {
        let point = pos(v)?;
        Some(VertexHit { vertex: v, point, verified: false, on_point: true })
    };
    let Some((_, cands)) = best else {
        let picked = if loose {
            resuffixed_vertex(record, incident, &faces, &wanted_parsed, reach, near)
        } else {
            renamed_vertex(record, incident, &faces, &wanted, reach, near)
        };
        return picked.and_then(at_pick).ok_or_else(|| format!("vertex `{name}` no longer exists"));
    };
    let vertex_near = |v: usize| pos(v).is_some_and(|p| near.is_finite() && p.dist(near) <= reach);
    match cands.as_slice() {
        [v] if numbered && !vertex_near(*v) => Err(format!("vertex `{name}` was one of several between its faces, and the one left is elsewhere")),
        [v] => hit(*v),
        _ => resuffixed_vertex(record, incident, &faces, &wanted_parsed, reach, near)
            .and_then(at_pick)
            .ok_or_else(|| format!("vertex `{name}` is no longer one vertex")),
    }
}

/// Find a saved Sketch Project vertex in its body, recovering numbered face pieces when the
/// faces around it now evaluate whole at the saved point.
pub(crate) fn locate_projected_vertex(b: &ModelBody, name: &str, near: Vec3) -> Result<VertexHit, String> {
    locate_vertex(b, name, near).or_else(|miss| recover_numbered_projected_vertex(b, name, near).ok_or(miss))
}

fn recover_numbered_projected_vertex(b: &ModelBody, name: &str, near: Vec3) -> Option<VertexHit> {
    if b.body.is_mesh() || !near.is_finite() {
        return None;
    }
    let (wanted, _) = parse_vertex_wanted(name);
    let mut numbered = false;
    let whole: Vec<&str> = wanted
        .iter()
        .map(|part| match part.rsplit_once('#') {
            Some((stem, number))
                if !stem.is_empty()
                    && strip_piece(stem) == stem
                    && !number.is_empty()
                    && !number.starts_with('0')
                    && number.bytes().all(|digit| digit.is_ascii_digit()) =>
            {
                numbered = true;
                stem
            }
            _ => part.as_str(),
        })
        .collect();
    if !numbered {
        return None;
    }
    let hit = locate_vertex(b, &whole.join("|"), near).ok().filter(|hit| hit.verified && hit.on_point)?;
    whole.iter().all(|face| faces_with_stem(b, face).len() == 1).then_some(hit)
}

fn parse_vertex_wanted(name: &str) -> (Vec<String>, bool) {
    if let Ok(parsed) = crate::persistent_name::EdgeName::parse(name) {
        return (parsed.faces.iter().map(ToString::to_string).collect(), parsed.tie.is_some());
    }
    let base = strip_edge_number(name);
    (split_top_level_bars(base).into_iter().map(str::to_string).collect(), base.len() != name.len())
}

fn split_top_level_bars(s: &str) -> Vec<&str> {
    let (mut parts, mut start, mut depth) = (Vec::new(), 0usize, 0usize);
    for (i, b) in s.bytes().enumerate() {
        match b {
            b'(' | b'[' => depth = depth.saturating_add(1),
            b')' | b']' => depth = depth.saturating_sub(1),
            b'|' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Best 1-to-1 bottleneck match between a vertex's incident faces `fs` and the wanted face list
/// `wanted`, maximizing the weakest individual face match.
fn vertex_match(faces: &Names, fs: &[u32], wanted: &[(String, Option<crate::persistent_name::FaceName>)]) -> Option<Match> {
    const MAX_INCIDENT: usize = 64;
    let k = wanted.len();
    if k == 0 || fs.len() != k || k > MAX_INCIDENT {
        return None;
    }
    let mut rows: Vec<Vec<(usize, Match)>> = Vec::with_capacity(k);
    let mut col_hit = vec![false; k];
    for (w_str, w_parsed) in wanted {
        let mut opts = Vec::new();
        for (ci, f) in fs.iter().enumerate() {
            if let Some(m) = crate::history_names::matches_read(faces, *f as usize, w_str, w_parsed.clone()) {
                col_hit[ci] = true;
                opts.push((ci, m));
            }
        }
        if opts.is_empty() {
            return None;
        }
        opts.sort_by_key(|a| std::cmp::Reverse(a.1));
        rows.push(opts);
    }
    if col_hit.iter().any(|hit| !*hit) {
        return None;
    }
    // Search fewest-option rows first so high-degree polygon corners resolve in linear time.
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by_key(|&r| rows[r].len());
    let mut used = vec![false; k];
    let mut best: Option<Match> = None;
    match_assign(&rows, &order, 0, &mut used, None, &mut best);
    best
}

fn match_assign(rows: &[Vec<(usize, Match)>], order: &[usize], depth: usize, used: &mut [bool], cur_min: Option<Match>, best: &mut Option<Match>) {
    if depth == order.len() {
        *best = (*best).max(cur_min);
        return;
    }
    let row = &rows[order[depth]];
    for &(col, level) in row {
        if used[col] {
            continue;
        }
        let next_min = Some(cur_min.map_or(level, |m| m.min(level)));
        if best.is_some_and(|b| next_min.is_some_and(|n| n <= b)) {
            continue;
        }
        used[col] = true;
        match_assign(rows, order, depth + 1, used, next_min, best);
        used[col] = false;
    }
}

fn resuffixed_vertex(
    record: &solidcore_record::brep::Brep,
    incident: &[Vec<u32>],
    faces: &Names,
    wanted: &[(String, Option<crate::persistent_name::FaceName>)],
    reach: f64,
    near: Vec3,
) -> Option<usize> {
    if !near.is_finite() || wanted.is_empty() {
        return None;
    }
    let side = |f: usize, (x, x_parsed): &(String, Option<crate::persistent_name::FaceName>)| -> Option<bool> {
        let level = crate::history_names::matches_read(faces, f, x, x_parsed.clone())?;
        if exactly(level) {
            let resuffixed = level == Match::Piece && qualified(x) && faces.faces.get(f).is_some_and(|n| !crate::history_names::descends(n, x));
            return Some(resuffixed);
        }
        (level == Match::Stem && qualified(x) && (faces.faces.get(f).is_some_and(|n| qualified(n)) || unique_resuffixed_on_body(faces, x) == Some(f)))
            .then_some(true)
    };
    let mut on = incident.iter().enumerate().filter_map(|(v, fs)| {
        let pt = record.vertices.get(v).map(|v| Vec3::new(v.point[0], v.point[1], v.point[2]))?;
        if !pt.is_finite() || pt.dist(near) > reach || fs.len() != wanted.len() || fs.len() > 64 {
            return None;
        }
        let mut rows: Vec<Vec<(usize, bool)>> = Vec::with_capacity(wanted.len());
        for w in wanted {
            let opts: Vec<(usize, bool)> = fs.iter().enumerate().filter_map(|(ci, f)| side(*f as usize, w).map(|r| (ci, r))).collect();
            if opts.is_empty() {
                return None;
            }
            rows.push(opts);
        }
        let mut used = vec![false; fs.len()];
        fn can_assign(rows: &[Vec<(usize, bool)>], depth: usize, used: &mut [bool], saw_resuffixed: bool) -> bool {
            if depth == rows.len() {
                return saw_resuffixed;
            }
            for &(col, r) in &rows[depth] {
                if !used[col] {
                    used[col] = true;
                    let ok = can_assign(rows, depth + 1, used, saw_resuffixed || r);
                    used[col] = false;
                    if ok {
                        return true;
                    }
                }
            }
            false
        }
        can_assign(&rows, 0, &mut used, false).then_some(v)
    });
    on.next().filter(|_| on.next().is_none())
}

fn renamed_vertex(
    record: &solidcore_record::brep::Brep,
    incident: &[Vec<u32>],
    faces: &Names,
    wanted: &[String],
    reach: f64,
    near: Vec3,
) -> Option<usize> {
    if !near.is_finite() || wanted.len() < 2 {
        return None;
    }
    let count = faces.faces.len();
    let found = |x: &str| {
        let mut hits = (0..count).filter(|f| crate::history_names::matches(faces, *f, x).is_some_and(exactly));
        hits.next().filter(|_| hits.next().is_none())
    };
    let lost = |y: &str| {
        crate::persistent_name::made_by(y)
            .filter(|id| faces.faces.iter().any(|f| crate::persistent_name::made_by(f) == Some(*id)))
            .filter(|_| (0..count).all(|f| crate::history_names::matches(faces, f, y).is_none()))
    };
    // Exactly one face is lost while its feature still exists on the body, and all remaining faces are found.
    let mut kept_faces: Vec<usize> = Vec::with_capacity(wanted.len() - 1);
    let mut lost_by: Option<u64> = None;
    for w in wanted {
        if let Some(f) = found(w)
            && !kept_faces.contains(&f)
        {
            kept_faces.push(f);
        } else if lost_by.is_none()
            && let Some(id) = lost(w)
        {
            lost_by = Some(id);
        } else {
            return None;
        }
    }
    let lost_by = lost_by?;
    let made_by = |f: usize| faces.faces.get(f).and_then(|name| crate::persistent_name::made_by(name));
    let mut on = incident.iter().enumerate().filter_map(|(v, fs)| {
        let pt = record.vertices.get(v).map(|v| Vec3::new(v.point[0], v.point[1], v.point[2]))?;
        if !pt.is_finite() || pt.dist(near) > reach || fs.len() != wanted.len() {
            return None;
        }
        let bounds_kept = kept_faces.iter().all(|k| fs.iter().any(|f| *f as usize == *k));
        let replaced = fs.iter().any(|f| !kept_faces.contains(&(*f as usize)) && made_by(*f as usize) == Some(lost_by));
        (bounds_kept && !replaced).then_some(v)
    });
    on.next().filter(|_| on.next().is_none())
}

/// Whether a face found at `level` is exactly the face a name named: the very name (or another
/// name of that very face: a face its feature made from several roles has each), or the named
/// piece told apart by the faces around it. A face the named one was merged into, or a face of
/// the same stem, is not.
fn exactly(level: Match) -> bool {
    matches!(level, Match::Exact | Match::Piece)
}

/// How far from an edge a picked point may lie and still be on it.
pub(crate) fn pick_reach(b: &ModelBody) -> f64 {
    (b.body.size() * 2e-3).max(1e-3)
}

/// Whether the picked point `near` lies on edge `i` of body `b` (whose mesh is `m`): then a
/// reference keeps it.
fn edge_near(b: &ModelBody, m: &Mesh, i: usize, near: Vec3) -> bool {
    m.edges.get(i).is_some_and(|e| e.windows(2).any(|w| near.dist_to_segment(w[0], w[1]) < pick_reach(b)))
}

/// The edge of body `b` whose exact curve passes through the picked point `near`, by its index
/// in the body's mesh `m` (the mesh's edges are the B-rep's, in its order: the edge's faces are
/// checked too). A point at an end of the edge is on the edges that meet there as well, so it
/// names none. `None` too for a mesh body, a point that is not finite, or a point a pick's reach
/// from every edge but a periodic face's seam or a pole.
fn edge_through(b: &ModelBody, m: &Mesh, near: Vec3) -> Option<usize> {
    let reach = pick_reach(b);
    let (d, i) = solidcraft_kernel::comb_edge_near(&b.body, near).ok().flatten()?;
    let record = b.body.record();
    let edge = record.edges.get(i)?;
    let sorted = |mut fs: Vec<u32>| {
        fs.sort_unstable();
        fs.dedup();
        fs
    };
    let same_faces = sorted(edge.uses.iter().map(|u| u.face).collect()) == sorted(m.edge_faces.get(i)?.clone());
    let clear_of_ends = edge
        .vertices
        .iter()
        .all(|v| record.vertices.get(*v as usize).is_some_and(|v| Vec3::new(v.point[0], v.point[1], v.point[2]).dist(near) > reach));
    (d <= reach && same_faces && clear_of_ends).then_some(i)
}

/// The edge a stored edge name `wanted` (without its number) meant when no edge's faces answer to
/// it at all: one of its two faces is still found exactly, the other finds no face although the
/// feature that named it still made faces of body `b`, and the reference's point `near` lies on an
/// edge of the face still found, clear of its ends (see [`edge_through`]). Then the point says
/// which edge this is: that feature may have named the face differently once (a design saved when
/// a round wall took the name of a later join's face), and the face may also be gone while the
/// feature stays (a shorter tab); either way the edge the pick lies on is taken. Not where that
/// edge's other face is one the same feature made: its face there was replaced (a sketch line
/// swapped for another), and the reference asks again. A face whose feature made nothing on the
/// body was removed with it, and so is the name. The point must be the
/// reference's own pick: a caller that passes a point on another edge cannot take what this finds.
/// Whether feature `fid` that made/modified body `b` is a revolve feature.
fn feature_is_revolve(b: &ModelBody, fid: u64) -> bool {
    let mut origin = b.names.origin.as_deref();
    while let Some(o) = origin {
        if o.feature.id == fid {
            return matches!(o.feature.kind, FeatureKind::Revolve { .. } | FeatureKind::SurfaceRevolve { .. });
        }
        origin = o
            .before
            .bodies
            .iter()
            .find(|prev| prev.name == b.name)
            .and_then(|prev| prev.names.origin.as_deref())
            .or_else(|| o.before.bodies.iter().find_map(|prev| prev.names.origin.as_deref()));
    }
    false
}

fn renamed_edge(b: &ModelBody, m: &Mesh, faces: &Names, wanted: &str, near: Vec3) -> Option<usize> {
    let count = faces.faces.len();
    let found = |x: &str| {
        let mut hits = (0..count).filter(|f| crate::history_names::matches(faces, *f, x).is_some_and(exactly));
        hits.next().filter(|_| hits.next().is_none())
    };
    let lost = |y: &str| {
        let id = crate::persistent_name::made_by(y)?;
        let has_feature = faces.faces.iter().any(|f| crate::persistent_name::made_by(f) == Some(id));
        if !has_feature {
            return None;
        }
        let is_revolve = feature_is_revolve(b, id);
        let lost_face = if is_revolve {
            (0..count).all(|f| !crate::history_names::matches(faces, f, y).is_some_and(exactly))
        } else {
            (0..count).all(|f| crate::history_names::matches(faces, f, y).is_none())
        };
        lost_face.then_some(id)
    };
    // Every way the name splits into two face names (a face name may hold `|` itself); they must
    // agree on the face still found.
    let mut kept: Vec<(usize, u64, &str)> = Vec::new();
    for (i, _) in wanted.match_indices('|') {
        let (Some(x), Some(y)) = (wanted.get(..i), wanted.get(i + 1..)) else { continue };
        for (x, y) in [(x, y), (y, x)] {
            if let Some((hit_f, hit_id)) = found(x).filter(|f| kept.iter().all(|(k, _, _)| k != f)).zip(lost(y)) {
                kept.push((hit_f, hit_id, y));
            }
        }
    }
    let &[(face, lost_by, lost_name)] = kept.as_slice() else { return None };
    let edge = edge_through(b, m, near)?;
    let uses = &b.body.record().edges.get(edge)?.uses;
    let made_by = |f: usize| faces.faces.get(f).and_then(|name| crate::persistent_name::made_by(name));
    let bounds_face = uses.iter().any(|u| u.face as usize == face);
    let other = uses.iter().find(|u| u.face as usize != face).map(|u| u.face as usize);
    let same_feature = other.and_then(made_by) == Some(lost_by);
    let is_revolve = feature_is_revolve(b, lost_by);
    let side_migration =
        same_feature && is_revolve && (qualified(lost_name) || other.and_then(|f| faces.faces.get(f)).is_some_and(|n| n.contains(":side:")));
    let replaced = same_feature && !side_migration;
    (bounds_face && !replaced).then_some(edge)
}

/// The edge a stored edge name `wanted` (without its number) meant when one of its faces is named
/// with the faces around it (`F44:top#[F43:side:l7]`) and those are now others: the face is still
/// there under its stem, but its qualifier changed, or several faces answer to it (a join over
/// another extrude's footprint names the walls it joins differently, and can leave two faces of
/// that stem). The candidates are the edges between faces answering to the name's two faces: each
/// exactly (see [`exactly`]) or, for a face named with its qualifier, by its stem where that face
/// too is named with a qualifier (another one: a face whole again is not it) or where the name's
/// qualifier lists only faces named by their index and the face borders such a face that is not
/// verified (those tell no piece apart now, so the piece has no qualifier), at least one
/// side of them by a qualifier that is not that face's own (a piece of the named face split since
/// is not a renamed face: the edge split with it and the name asks again). Of those, the one edge
/// whose exact curve the reference's point `near` lies on (within the pick reach, as
/// [`solidcraft_kernel::edge_distance`] measures it) is taken; none or several, and the name finds
/// nothing. The point must be the reference's own pick, as for [`renamed_edge`].
fn resuffixed_edge(b: &ModelBody, m: &Mesh, faces: &Names, wanted: &str, near: Vec3) -> Option<usize> {
    if b.body.is_mesh() || !near.is_finite() {
        return None;
    }
    let beside_index = beside_index_named(b, faces);
    let mut resuffixed_cache: std::collections::HashMap<&str, Option<usize>> = std::collections::HashMap::new();
    for (k, _) in wanted.match_indices('|') {
        for part in [wanted.get(..k), wanted.get(k + 1..)].into_iter().flatten() {
            if qualified(part) {
                resuffixed_cache.entry(part).or_insert_with(|| unique_resuffixed_on_body(faces, part));
            }
        }
    }
    // How face `f` answers to face name `x`: `Some(true)` by a qualifier that is not its own.
    let side = |f: usize, x: &str| -> Option<bool> {
        let level = crate::history_names::matches(faces, f, x)?;
        if exactly(level) {
            let resuffixed = level == Match::Piece && qualified(x) && faces.faces.get(f).is_some_and(|n| !crate::history_names::descends(n, x));
            return Some(resuffixed);
        }
        // By its stem only where the face is still named after its neighbours, others than the name's:
        // a face whole again is not one the name told apart by a face it never touches (#11731).
        // A name told apart only by faces named by their index may name a piece that has no
        // qualifier now, where the face still borders such an unverified face: those no longer
        // tell pieces apart (#19455).
        let unqualified_piece = || told_apart_by_index_only(x) && beside_index.get(f).copied().unwrap_or(false);
        (level == Match::Stem
            && qualified(x)
            && (faces.faces.get(f).is_some_and(|n| qualified(n) || unqualified_piece()) || resuffixed_cache.get(x).copied().flatten() == Some(f)))
        .then_some(true)
    };
    let mut cands: Vec<usize> = Vec::new();
    for (i, fs) in m.edge_faces.iter().enumerate() {
        let [fa, fb] = fs.as_slice() else { continue };
        let answers = wanted.match_indices('|').any(|(k, _)| {
            let (Some(x), Some(y)) = (wanted.get(..k), wanted.get(k + 1..)) else { return false };
            [(*fa, *fb), (*fb, *fa)].into_iter().any(|(p, q)| side(p as usize, x).zip(side(q as usize, y)).is_some_and(|(sx, sy)| sx || sy))
        });
        if answers {
            cands.push(i);
        }
    }
    let reach = pick_reach(b);
    let record = b.body.record();
    let sorted = |mut fs: Vec<u32>| {
        fs.sort_unstable();
        fs.dedup();
        fs
    };
    let mut on = cands.into_iter().filter(|i| {
        // The mesh's edges are the B-rep's, in its order: the edge's faces say it is the same edge.
        let same = record
            .edges
            .get(*i)
            .zip(m.edge_faces.get(*i))
            .is_some_and(|(e, fs)| sorted(e.uses.iter().map(|u| u.face).collect()) == sorted(fs.clone()));
        same && solidcraft_kernel::edge_distance(&b.body, *i, near).ok().flatten().is_some_and(|d| d <= reach)
    });
    on.next().filter(|_| on.next().is_none())
}

/// Whether face name `x` names its face with the faces around it (`F2:top#[F4:side:l1]`).
fn qualified(x: &str) -> bool {
    crate::piece_names::piece_parts(x).is_some_and(|(_, around)| !around.is_empty())
}

/// Whether face name `x` is told apart only by faces named by their index
/// (`F5:side:l1#[F2:face:6,F3:face:7]`, a sphere's faces). Names saved before unverified faces
/// named by their index stopped telling pieces apart hold them; the piece they named can be named
/// without them now.
fn told_apart_by_index_only(x: &str) -> bool {
    crate::piece_names::piece_parts(x)
        .is_some_and(|(_, around)| !around.is_empty() && around.iter().all(|a| crate::history_names::named_by_index(strip_piece(a))))
}

/// For each face of body `b` (named by `faces`), whether it borders a face named only by its index
/// and not verified (a sphere's, or a sweep's past a bend): such a neighbour tells no piece apart
/// (#18481), so a piece it bounds may be named without the qualifier a design saved before held.
/// Read from the B-rep's edges and the faces each edge's uses lie on.
fn beside_index_named(b: &ModelBody, faces: &Names) -> Vec<bool> {
    let mut beside = vec![false; faces.faces.len()];
    let by_index = |g: usize| !faces.verified(g) && faces.faces.get(g).is_some_and(|n| crate::history_names::named_by_index(strip_piece(n)));
    for edge in &b.body.record().edges {
        for p in &edge.uses {
            if edge.uses.iter().any(|q| q.face != p.face && by_index(q.face as usize))
                && let Some(slot) = beside.get_mut(p.face as usize)
            {
                *slot = true;
            }
        }
    }
    beside
}

/// The one face among `faces` (each answering to the stored name `name` as a piece, on body `b`)
/// that is the piece the name told apart, when that piece lost its qualifier: a design saved before
/// faces named by their index stopped telling pieces apart (#18481) named the piece after neighbours
/// that still bound it, and the piece is now named by its bare stem because the faces that told it
/// apart from its siblings are named by their index. Such a sibling still answers to the name too
/// (it is a piece of the same stem bounded by the same faces). The piece is the face named by the
/// name's bare stem that borders an unverified face named by its index and that the reference's
/// point `p` lies on (exactly, within the pick reach); none or several, and the name finds nothing
/// (#20027).
fn unqualified_piece(b: &ModelBody, names: &Names, faces: &[usize], name: &str, p: Vec3) -> Option<(usize, Vec3)> {
    let (stem, around) = crate::piece_names::piece_parts(name)?;
    if around.is_empty() || !p.is_finite() {
        return None;
    }
    let beside = beside_index_named(b, names);
    let mut on = faces.iter().filter_map(|&f| {
        let bare = names.faces.get(f).is_some_and(|own| own == stem);
        (bare && beside.get(f).copied().unwrap_or(false)).then(|| on_face(b, f, p)).flatten()
    });
    on.next().filter(|_| on.next().is_none())
}

/// The point of face `face` of a mesh nearest `p`, and how far it is.
fn nearest_on_face(m: &Mesh, face: usize, p: Vec3) -> Option<(f64, Vec3)> {
    let mut best: Option<(f64, Vec3)> = None;
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        if *f as usize != face {
            continue;
        }
        let Some([a, b, c]) = m.tri(t) else { continue };
        let q = closest(p, a, b, c);
        let d = q.dist(p);
        // A non-finite candidate must never become the best: nothing compares less than NaN.
        if d.is_finite() && best.is_none_or(|x| d < x.0) {
            best = Some((d, q));
        }
    }
    best
}

/// The middle of edge `i` of a mesh (along its polyline).
fn edge_mid(m: &Mesh, i: usize) -> Option<Vec3> {
    let e = m.edges.get(i)?;
    let total: f64 = e.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut acc = 0.0;
    for w in e.windows(2) {
        let l = w[0].dist(w[1]);
        if acc + l >= total / 2.0 {
            let t = if l > 0.0 { (total / 2.0 - acc) / l } else { 0.0 };
            return Some(w[0] + (w[1] - w[0]) * t);
        }
        acc += l;
    }
    e.first().copied()
}

/// How well the stored edge name `wanted` (without its own number) names the edge between faces
/// `a` and `b`: the weaker of its two faces' matches, read every way the name splits into two
/// face names (a face name may hold `|` itself: a fillet face is named after its edge).
fn edge_match(faces: &Names, a: usize, b: usize, wanted: &str) -> Option<Match> {
    let mut best = None;
    for (i, _) in wanted.match_indices('|') {
        let (Some(x), Some(y)) = (wanted.get(..i), wanted.get(i + 1..)) else { continue };
        for (p, q) in [(a, b), (b, a)] {
            if let (Some(m1), Some(m2)) = (crate::history_names::matches(faces, p, x), crate::history_names::matches(faces, q, y)) {
                best = best.max(Some(m1.min(m2)));
            }
        }
    }
    best
}

/// Keep `item` among the best matches so far.
fn keep_best<T>(best: &mut Option<(Match, Vec<T>)>, level: Match, item: T) {
    match best {
        Some((b, items)) if *b == level => items.push(item),
        Some((b, _)) if *b > level => {}
        _ => *best = Some((level, vec![item])),
    }
}

/// Where a stored face name was found.
#[derive(Clone, Copy, Debug)]
pub struct FaceHit {
    /// The body (its index in the model's bodies).
    pub body: usize,
    /// The face (its index in the body).
    pub face: usize,
    /// The point on the face nearest the reference's point.
    pub point: Vec3,
    /// Whether the face's name follows exact history.
    pub verified: bool,
    /// Whether the reference's point still lies on the face (so a name worked out from geometry
    /// and the pick agree).
    pub on_point: bool,
}

/// Why a stored face name finds no face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Miss {
    /// No face has the name, or only a face it was merged into or a face of the same stem is
    /// left, or the name was saved before format 14.
    Gone,
    /// Several faces answer to it as well: the pieces of a split face, or the same face on
    /// several bodies.
    Several,
}

impl FaceHit {
    /// What a feature says about how it found a face by its stored name `name` (`None`: exactly).
    pub fn note(&self, name: &str) -> Option<String> {
        if !self.verified && !self.on_point {
            Some(format!("face `{}` was found by geometry, not exact history; check it", quoted(name)))
        } else {
            None
        }
    }
}

impl FaceHit {
    /// Where the line through `p` along `dir` meets the face found (the hit nearest `p`, on
    /// either side of it), `None` when the line misses the face or runs along it, or the body
    /// is gone; `Err` when the kernel fails to meet them. A place on a face follows the face
    /// this way, along its own axis, instead of sliding sideways to [`point`](Self::point), the
    /// face's nearest point. Decided on the face's exact surface and trim
    /// ([`solidcraft_kernel::Body::face_line_hit`]), never its display mesh: from a facet of a
    /// curved face a hole would start inside the surface (#20741).
    pub fn along(&self, st: &ModelState, p: Vec3, dir: Vec3) -> solidcraft_kernel::Result<Option<Vec3>> {
        let Some(b) = st.bodies.get(self.body) else { return Ok(None) };
        b.body.face_line_hit(self.face, p, dir)
    }

    /// A point that picks the face found again by itself: [`point`](Self::point) when the
    /// reference's point still lies on the face clear of its edges; else the point of the face
    /// nearest `p` that keeps clear of every edge of it. The nearest point of a face the
    /// reference's point no longer lies on is on the face's boundary (a parameter moved the face
    /// away from the pick), where a pick by point finds the face beside it as well (#2531).
    /// The chosen interior point is projected onto the exact trimmed face when that finds it (a
    /// curved face's triangle lies off its surface by the display deviation); when the exact
    /// query finds nothing the unprojected point is returned, as it always was.
    pub fn pick_point(&self, st: &ModelState, p: Vec3) -> Vec3 {
        let Some(b) = st.bodies.get(self.body) else { return self.point };
        let m = b.mesh();
        // Clear of the edges by a few pick tolerances, or by half the face's roomiest triangle
        // on a face narrower than that.
        let margin = roomiest_inradius(&m, self.face).map(|r| (4.0 * (b.body.size() * 1e-3).max(1e-3)).min(r * 0.5));
        let point = match margin {
            Some(margin) if self.on_point && edge_clearance(&m, self.face, self.point) >= margin => self.point,
            Some(margin) => inside_nearest(&m, self.face, p, margin).unwrap_or(self.point),
            None => self.point,
        };
        match solidcraft_kernel::point_closest(point, &b.body, solidcraft_kernel::Members::Faces(&[self.face])) {
            Ok(Some(nearest)) => nearest.to,
            _ => point,
        }
    }
}

/// The incentre and inradius of triangle abc (`None`: degenerate).
fn incircle(a: Vec3, b: Vec3, c: Vec3) -> Option<(Vec3, f64)> {
    // The incentre weighs each corner by the side opposite it; the inradius is area / half the perimeter.
    let (la, lb, lc) = (b.dist(c), c.dist(a), a.dist(b));
    let perimeter = la + lb + lc;
    let radius = (b - a).cross(c - a).len() / perimeter;
    let centre = (a * la + b * lb + c * lc) / perimeter;
    (radius.is_finite() && radius > 0.0 && centre.is_finite()).then_some((centre, radius))
}

/// The triangles of face `face` of a mesh.
fn face_triangles(m: &Mesh, face: usize) -> impl Iterator<Item = [Vec3; 3]> + '_ {
    m.triangles.iter().zip(&m.tri_face).filter(move |(_, f)| **f as usize == face).filter_map(|(t, _)| m.tri(t))
}

/// The greatest inradius of the triangles of face `face` of a mesh (`None`: it has none).
fn roomiest_inradius(m: &Mesh, face: usize) -> Option<f64> {
    face_triangles(m, face).filter_map(|[a, b, c]| incircle(a, b, c)).map(|(_, r)| r).reduce(f64::max)
}

/// How far `q` is from the nearest edge bounding face `face` of a mesh (infinite: it has none).
fn edge_clearance(m: &Mesh, face: usize, q: Vec3) -> f64 {
    m.edges
        .iter()
        .zip(&m.edge_faces)
        .filter(|(_, fs)| fs.iter().any(|f| *f as usize == face))
        .flat_map(|(e, _)| e.windows(2).map(|w| q.dist_to_segment(w[0], w[1])))
        .filter(|d| d.is_finite())
        .fold(f64::INFINITY, f64::min)
}

/// The point of face `face` of a mesh nearest `p` among the points at least `margin` inside one
/// of its triangles (each triangle shrunk about its incentre by `margin`): such a point is at
/// least `margin` from every edge of the face, which runs along the sides of its triangles.
/// `None`: no triangle of the face is roomier than `margin`.
fn inside_nearest(m: &Mesh, face: usize, p: Vec3, margin: f64) -> Option<Vec3> {
    let mut best: Option<(f64, Vec3)> = None;
    for [a, b, c] in face_triangles(m, face) {
        let Some((centre, radius)) = incircle(a, b, c).filter(|(_, r)| *r > margin) else { continue };
        // Shrunk about the incentre so each side moves in by `margin`.
        let s = (radius - margin) / radius;
        let shrink = |v: Vec3| centre + (v - centre) * s;
        let q = closest(p, shrink(a), shrink(b), shrink(c));
        let d = q.dist(p);
        if d.is_finite() && best.is_none_or(|(old, _)| d < old) {
            best = Some((d, q));
        }
    }
    best.map(|(_, q)| q)
}

/// A stored name as a message quotes it: whole up to about 80 characters, else its start and
/// end around an ellipsis. Generated names grow with the history (a split edge's name holds both
/// its faces' names), and a chamfer's warning that quoted fourteen of them in full ran to 122,701
/// characters (#8541).
pub(crate) fn quoted(name: &str) -> String {
    const HEAD: usize = 48;
    const TAIL: usize = 24;
    let n = name.chars().count();
    if n <= HEAD + TAIL + 8 {
        return name.to_string();
    }
    let head: String = name.chars().take(HEAD).collect();
    let tail: String = name.chars().skip(n - TAIL).collect();
    format!("{head}…{tail}")
}

impl EdgeHit {
    /// What a feature says about how it found an edge by its stored name `name` (`None`: exactly).
    pub fn note(&self, name: &str) -> Option<String> {
        if !self.verified && !self.on_point {
            Some(format!("edge `{}` was found by geometry, not exact history; check it", quoted(name)))
        } else {
            None
        }
    }
}

/// The face a stored face name finds, on any body: the face with that name, or the named piece
/// told apart by the faces around it, or a numbered piece of a face that is whole now (see
/// [`recover_numbered_face_in`]). `None` when no face does, or several do.
pub fn locate_face(st: &ModelState, name: &str, p: Vec3) -> Option<FaceHit> {
    locate_face_with(st, name, p, false)
}

fn locate_face_with(st: &ModelState, name: &str, p: Vec3, round: bool) -> Option<FaceHit> {
    // A `sheet:<body>:<panel>:<side>` name a design saved before keeps finds no face: no sheet
    // names a face so now (see [`crate::sheet::sheet_face_name`]).
    if crate::sheet::split_sheet_face_name(name).is_some() || legacy_name(name).is_some() {
        return None;
    }
    // Every body's names are read below: work out the ones not known yet on all cores first
    // (each body's names are its own, whichever thread works them out).
    naming::warm_names(st.bodies.iter().filter(|b| !b.body.is_mesh()));
    match locate_in(st.bodies.iter().enumerate().filter(|(_, b)| !b.body.is_mesh()), name, Some(p)) {
        Ok(hit) => Some(hit),
        Err(Miss::Gone) => recover_numbered_face_in(st, name, p, round)
            .or_else(|| replaced_side_line(st, name, p))
            .or_else(|| numbered_sheet_face(st, name, p))
            .or_else(|| recover_positional_face(st, name, p)),
        Err(Miss::Several) => None,
    }
}

/// The `F2` of a face named by its number among the faces feature `F2` made (`F2:face:1`): `None`
/// for any other name.
fn numbered_face_name(name: &str) -> Option<&str> {
    let (stem, k) = name.split_once(":face:")?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    (stem.strip_prefix('F').is_some_and(digits) && digits(k)).then_some(stem)
}

/// A face of a sheet saved by its number among the faces its feature made (`F2:face:1`), as builds
/// before the sheet was made from surfaces (#6748) named every face of a sheet feature. A sheet
/// feature names its faces after what made each now (`F2:top`, `F2:side:l1`,
/// `F3:far:F2:side:l1`), and numbers none, so the stored name names nothing. The stored point
/// still lies on the face it was: that face, when it is the one face of a sheet body that the same
/// feature made and the point lies on. A point on no face of that feature, or on two, finds
/// nothing, as before; so does a name of a feature that still numbers a face of the body.
fn numbered_sheet_face(st: &ModelState, name: &str, p: Vec3) -> Option<FaceHit> {
    let stem = numbered_face_name(name)?;
    if !p.is_finite() {
        return None;
    }
    let made = |own: &str| own.split_once(':').is_some_and(|(s, _)| s == stem);
    let sheet = |b: &ModelBody| st.surface_sheets.iter().any(|s| s.body == b.name);
    let mut found: Option<FaceHit> = None;
    for (bi, b) in st.bodies.iter().enumerate().filter(|(_, b)| !b.body.is_mesh() && sheet(b) && b.body.face_count() <= MAX_PROJECTION_RECOVERY_FACES)
    {
        let names = naming::names(b);
        if names.faces.iter().any(|own| numbered_face_name(own) == Some(stem)) {
            continue;
        }
        for (face, own) in names.faces.iter().enumerate() {
            if !made(own.as_str()) {
                continue;
            }
            let Some((_, point)) = on_face(b, face, p) else { continue };
            if found.is_some() {
                return None;
            }
            found = Some(FaceHit { body: bi, face, point, verified: names.verified(face), on_point: true });
        }
    }
    found
}

/// The `F2` and the number of a side face named after a sketch line (`F2:side:l1`): `None` for any
/// other name (a numbered piece, a face between neighbours, an arc's side).
fn side_line_name(name: &str) -> Option<(&str, &str)> {
    let (stem, id) = name.split_once(":side:l")?;
    (!stem.is_empty() && !stem.contains(':') && !id.is_empty() && id.bytes().all(|c| c.is_ascii_digit())).then_some((stem, id))
}

/// A side face whose sketch line was replaced (#16661): the line was deleted and another drawn
/// where it was, so the new line's side face has a new name (`F2:side:l5`) and the stored one
/// (`F2:side:l1`) names nothing. The stored point still lies on the face of the line now there:
/// that face, when it is the one side face of the same feature the point lies on. A line drawn
/// anywhere else leaves the point on no side face, and the name finds nothing, as before.
fn replaced_side_line(st: &ModelState, name: &str, p: Vec3) -> Option<FaceHit> {
    let (stem, _) = side_line_name(name)?;
    if !p.is_finite() {
        return None;
    }
    let mut found: Option<FaceHit> = None;
    for (bi, b) in st.bodies.iter().enumerate().filter(|(_, b)| !b.body.is_mesh() && b.body.face_count() <= MAX_PROJECTION_RECOVERY_FACES) {
        let names = naming::names(b);
        for (face, own) in names.faces.iter().enumerate() {
            if side_line_name(own).is_none_or(|(s, _)| s != stem) {
                continue;
            }
            let Some((_, point)) = on_face(b, face, p) else { continue };
            if found.is_some() {
                return None;
            }
            found = Some(FaceHit { body: bi, face, point, verified: names.verified(face), on_point: true });
        }
    }
    found
}

/// A numbered piece of a round face (`F2:side:c1#2`, from the kernel before SMLib) on whichever
/// one body recovers it as its whole face (#13252): see [`recover_numbered_projected_face`] for
/// when that is safe. `None` when no body, or more than one, does.
fn recover_numbered_face_in(st: &ModelState, name: &str, p: Vec3, round: bool) -> Option<FaceHit> {
    if !name.contains('#') && !(round && name.contains(":side:")) {
        return None;
    }
    let mut found = None;
    for (body, b) in st.bodies.iter().enumerate().filter(|(_, b)| !b.body.is_mesh()) {
        let recovered = recover_numbered_projected_face(b, name, p).or_else(|| if round { recover_round_face(b, name, p) } else { None });
        if let Some((face, point)) = recovered {
            if found.is_some() {
                return None;
            }
            found = Some(FaceHit { body, face, point, verified: true, on_point: true });
        }
    }
    found
}

/// The one body of `st` with a face the stored face name finds, as [`locate_face`] looks for it.
/// `None` when no body has one, several do, or the name is one saved before format 14.
pub(crate) fn face_body<'a>(st: &'a ModelState, name: &str) -> Option<&'a ModelBody> {
    if let Some((body, _)) = crate::sheet::split_sheet_face_name(name) {
        return st.bodies.iter().find(|b| b.name == body && !b.body.is_mesh());
    }
    if name.is_empty() || legacy_name(name).is_some() {
        return None;
    }
    let solids = || st.bodies.iter().filter(|b| !b.body.is_mesh());
    naming::warm_names(solids());
    let mut exact = solids().filter(|b| body_has_exact_face_named(b, name));
    match (exact.next(), exact.next()) {
        (Some(b), None) => return Some(b),
        (Some(_), Some(_)) => return None,
        (None, _) => {}
    }
    let mut fallback = solids().filter(|b| unique_resuffixed_on_body(&naming::names(b), name).is_some());
    match (fallback.next(), fallback.next()) {
        (Some(b), None) => Some(b),
        _ => None,
    }
}

/// [`locate_face`] for a feature that works on the faces of the body called `body` (a shell, a
/// draft…): the face of that body the name finds, found without working out the names of every
/// other body (a body named from geometry is meshed for it: seconds in a large model). When
/// that body has no such face, as [`locate_face`] over every body: the name moved to another
/// body (a combine took the face). The same face on another body (a copy of this one) does not
/// make this body's face ambiguous.
pub fn locate_face_on_body(st: &ModelState, body: Option<&str>, name: &str, p: Vec3) -> Option<FaceHit> {
    locate_face_on_body_with(st, body, name, p, false)
}

/// [`locate_face_on_body`] with the recovery of a thread's face: a round side (cylinder or cone) saved by
/// 0.30.1 as a numbered piece (`F1:side#2`) or a round tool's wall under a later index
/// (`F4:bore:side:1`) is found as the whole face it is now, when it is the only face of its stem
/// and the saved pick lies on it (#16602). Only a thread asks for this: a thread wants the whole
/// face whatever piece it was saved on, while any other feature would take in material the old
/// piece never covered.
pub fn locate_thread_face_on_body(st: &ModelState, body: Option<&str>, name: &str, p: Vec3) -> Option<FaceHit> {
    locate_face_on_body_with(st, body, name, p, true)
}

/// Whether [`locate_face_on_body`] resolves `name` directly on `st.bodies[body_index]` without
/// falling back to scanning other bodies in `st`.
pub(crate) fn face_resolved_on_body(st: &ModelState, body_index: usize, name: &str, p: Vec3) -> bool {
    if crate::sheet::split_sheet_face_name(name).is_some() || legacy_name(name).is_some() {
        return false;
    }
    let Some(b) = st.bodies.get(body_index).filter(|b| !b.body.is_mesh()) else {
        return false;
    };
    locate_in(std::iter::once((body_index, b)), name, Some(p)).is_ok()
}

/// [`locate_face_on_body`] for a face that stands only for where it lies: an extrude's From or To
/// face. When the named face is now split into pieces (a letter with a closed hole cut into it
/// leaves an island of it, #22281), every piece lies on the face's own surface, so the piece the
/// saved point lies on stands for the face as well as the face did: see [`split_face_piece`].
/// A feature that works on the face's material (a shell, a draft) still finds nothing in a split
/// face: the piece under its point is not all it took.
pub fn locate_level_face_on_body(st: &ModelState, body: Option<&str>, name: &str, p: Vec3) -> Option<FaceHit> {
    locate_face_on_body(st, body, name, p).or_else(|| split_face_piece(st, name, p))
}

/// The one piece of a split face that point `p` lies on (exactly, as [`on_face`] decides), when
/// the stored name `name` is the whole face's (not a piece told apart by its neighbours, nor a name
/// saved before format 14), and all the faces answering to it are on one body. `None` when the
/// point lies on none of them or on several (an edge they share), or the pieces are on several
/// bodies.
fn split_face_piece(st: &ModelState, name: &str, p: Vec3) -> Option<FaceHit> {
    if name.is_empty() || !p.is_finite() || legacy_name(name).is_some() || crate::sheet::split_sheet_face_name(name).is_some() || qualified(name) {
        return None;
    }
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    for (bi, b) in st.bodies.iter().enumerate().filter(|(_, b)| !b.body.is_mesh()) {
        let names = naming::names(b);
        pieces.extend((0..names.faces.len()).filter(|&f| crate::history_names::matches(&names, f, name).is_some_and(exactly)).map(|f| (bi, f)));
    }
    let (bi, _) = *pieces.first()?;
    if pieces.len() < 2 || pieces.iter().any(|(other, _)| *other != bi) {
        return None;
    }
    let b = st.bodies.get(bi)?;
    let mut on = pieces.iter().filter_map(|&(_, f)| on_face(b, f, p));
    let (face, point) = on.next().filter(|_| on.next().is_none())?;
    Some(FaceHit { body: bi, face, point, verified: naming::names(b).verified(face), on_point: true })
}

fn locate_face_on_body_with(st: &ModelState, body: Option<&str>, name: &str, p: Vec3, round: bool) -> Option<FaceHit> {
    if crate::sheet::split_sheet_face_name(name).is_some() || legacy_name(name).is_some() {
        return locate_face_with(st, name, p, round);
    }
    if let Some(i) = body.and_then(|n| st.bodies.iter().position(|b| b.name == n))
        && let Some(b) = st.bodies.get(i).filter(|b| !b.body.is_mesh())
    {
        match locate_in(std::iter::once((i, b)), name, Some(p)) {
            Ok(hit) => return Some(hit),
            Err(Miss::Several) => return None,
            Err(Miss::Gone) => {}
        }
    }
    locate_face_with(st, name, p, round)
}

/// The one face of `names` that a neighbour-qualified piece name `name` still identifies after an
/// adjacent edit changed some of the faces around it: no face matches `name` exactly or as a full
/// piece, no other face of the same stem touches any face in `name`'s qualifier, and at least half
/// of the qualifier's faces still bound this one.
pub(crate) fn unique_resuffixed_on_body(names: &Names, name: &str) -> Option<usize> {
    if legacy_name(name).is_some() || !name.contains("#[") {
        return None;
    }
    let mut found: Option<(usize, usize, usize)> = None;
    for face in 0..names.faces.len() {
        match crate::history_names::matches(names, face, name) {
            Some(level) if exactly(level) => return None,
            Some(Match::Stem) => {
                let (matched, total) = crate::history_names::piece_overlap(names, face, name)?;
                if matched > 0 {
                    if found.is_some() {
                        return None;
                    }
                    found = Some((face, matched, total));
                }
            }
            _ => {}
        }
    }
    let (face, matched, total) = found?;
    (matched.saturating_mul(2) >= total).then_some(face)
}

/// The one face among `candidates` (each matching `name` by stem only, when no face matched
/// exactly or with its full qualifier) that a neighbour-qualified piece name still identifies
/// after an adjacent cut changed some of its neighbouring faces:
/// - when at most one face per body retains any neighbour from the stored qualifier, and at least
///   half of the qualifier's neighbours still bound it, that face is taken (even if it moved; when
///   copies on distinct bodies tie, the saved pick `pick` decides);
/// - otherwise, with a saved pick `pick`, the unique candidate whose exact surface contains `pick`
///   is taken — among those that still share a neighbour from the stored qualifier, or (when none
///   do, e.g. its sole qualifying neighbour was replaced) among faces that are still qualified
///   pieces, never a face that became whole again with zero surviving neighbours (#11731).
fn resuffixed_face<'a>(
    candidates: &[(usize, &'a ModelBody, usize)],
    name: &str,
    pick: Option<Vec3>,
    found: impl Fn((usize, &'a ModelBody, usize)) -> Option<FaceHit>,
) -> Option<FaceHit> {
    if candidates.is_empty() || !name.contains("#[") {
        return None;
    }
    let mut scored: Vec<(usize, &ModelBody, usize, usize, usize)> = Vec::with_capacity(candidates.len());
    for &(bi, b, face) in candidates {
        let names = naming::names(b);
        let (matched, total) = crate::history_names::piece_overlap(&names, face, name)?;
        scored.push((bi, b, face, matched, total));
    }
    let overlap: Vec<(usize, &ModelBody, usize, usize, usize)> = scored.iter().copied().filter(|&(_, _, _, m, _)| m > 0).collect();
    // If multiple faces on the same body still claim neighbours from the stored qualifier, the
    // piece was split (or became ambiguous): never pick one side silently.
    let mut seen_bodies = std::collections::BTreeSet::new();
    if overlap.iter().any(|&(bi, ..)| !seen_bodies.insert(bi)) {
        return None;
    }
    let majority: Vec<_> = overlap.iter().copied().filter(|&(_, _, _, m, total)| m.saturating_mul(2) >= total).collect();
    if majority.len() == overlap.len() && !majority.is_empty() {
        if let &[(bi, b, face, ..)] = majority.as_slice() {
            return found((bi, b, face));
        }
        let p = pick.filter(|p| p.is_finite())?;
        let mut on = majority.into_iter().filter(|(_, b, ..)| !b.body.is_mesh()).filter_map(|(bi, b, face, ..)| {
            let (_, point) = on_face(b, face, p)?;
            Some(FaceHit { body: bi, face, point, verified: naming::names(b).verified(face), on_point: true })
        });
        return on.next().filter(|_| on.next().is_none());
    }
    let p = pick.filter(|p| p.is_finite())?;
    let eligible: Vec<_> = if !overlap.is_empty() {
        overlap
    } else {
        scored.into_iter().filter(|&(_, b, face, ..)| naming::names(b).faces.get(face).is_some_and(|n| qualified(n))).collect()
    };
    let mut on = eligible.into_iter().filter(|(_, b, ..)| !b.body.is_mesh()).filter_map(|(bi, b, face, matched, _)| {
        let (_, point) = on_face(b, face, p)?;
        let verified = matched > 0 && naming::names(b).verified(face);
        Some(FaceHit { body: bi, face, point, verified, on_point: true })
    });
    on.next().filter(|_| on.next().is_none())
}

/// [`locate_face`] among `bodies` (with their indices): the one face the name finds there.
fn locate_in<'a>(bodies: impl Iterator<Item = (usize, &'a ModelBody)>, name: &str, pick: Option<Vec3>) -> Result<FaceHit, Miss> {
    if legacy_name(name).is_some() {
        return Err(Miss::Gone);
    }
    let p = pick.unwrap_or(Vec3::ZERO);
    let mut best: Option<(Match, Vec<(usize, &ModelBody, usize)>)> = None;
    let mut stem: Vec<(usize, &ModelBody, usize)> = Vec::new();
    for (bi, b) in bodies {
        let names = naming::names(b);
        for face in 0..names.faces.len() {
            match crate::history_names::matches(&names, face, name) {
                Some(level) if exactly(level) => keep_best(&mut best, level, (bi, b, face)),
                Some(Match::Stem) if best.is_none() => stem.push((bi, b, face)),
                _ => {}
            }
        }
    }
    let found = |(body, b, face): (usize, &ModelBody, usize)| {
        let (distance, point) = nearest_on_face(&b.mesh(), face, p)?;
        Some(FaceHit { body, face, point, verified: naming::names(b).verified(face), on_point: distance <= (b.body.size() * 1e-3).max(1e-3) })
    };
    let Some((level, faces)) = best else {
        return resuffixed_face(&stem, name, pick, found).ok_or(Miss::Gone);
    };
    if let [one] = faces.as_slice() {
        return found(*one).ok_or(Miss::Gone);
    }
    // The very face on several bodies (a body and its copies, whose faces keep their names): the
    // one the reference's point lies on. Pieces of one face are never told apart by a point.
    let mut bodies: Vec<usize> = faces.iter().map(|(bi, _, _)| *bi).collect();
    bodies.dedup();
    if level == Match::Piece
        && let [only] = bodies.as_slice()
        && let Some((_, b, _)) = faces.first()
        && let Some(p) = pick
    {
        // Pieces of one face on one body: the piece a design saved before #18481 told apart by
        // neighbours whose qualifier it lost since (#20027).
        let names = naming::names(b);
        let pieces: Vec<usize> = faces.iter().map(|(_, _, f)| *f).collect();
        return unqualified_piece(b, &names, &pieces, name, p)
            .map(|(face, point)| FaceHit { body: *only, face, point, verified: names.verified(face), on_point: true })
            .ok_or(Miss::Several);
    }
    if level != Match::Exact || bodies.len() < faces.len() || pick.is_none() {
        return Err(Miss::Several);
    }
    let mut on = faces.into_iter().filter_map(found).filter(|hit| hit.on_point);
    match (on.next(), on.next()) {
        (Some(hit), None) => Ok(hit),
        _ => Err(Miss::Several),
    }
}

/// Whether face `index` of a body is exactly the face a stored name names (see [`exactly`]). A
/// name saved before format 14 names no face.
pub fn face_has_name(b: &ModelBody, index: usize, name: &str) -> bool {
    if legacy_name(name).is_some() {
        return false;
    }
    let names = naming::names(b);
    match crate::history_names::matches(&names, index, name) {
        Some(level) if exactly(level) => true,
        Some(Match::Stem) => unique_resuffixed_on_body(&names, name) == Some(index),
        _ => false,
    }
}

/// Whether some face of a body matches the stored name directly (without neighbour-overlap
/// fallback).
fn body_has_exact_face_named(b: &ModelBody, name: &str) -> bool {
    let names = naming::names(b);
    (0..names.faces.len()).any(|i| crate::history_names::matches(&names, i, name).is_some_and(exactly))
}

/// Whether some face of a body is exactly the face a stored name names (see [`face_has_name`]).
pub(crate) fn body_has_face_named(b: &ModelBody, name: &str) -> bool {
    if legacy_name(name).is_some() {
        return false;
    }
    body_has_exact_face_named(b, name) || unique_resuffixed_on_body(&naming::names(b), name).is_some()
}

/// The faces of a body that are the stored name's face or pieces of it (by stem). A name saved
/// before format 14 has none.
pub fn faces_with_stem(b: &ModelBody, name: &str) -> Vec<usize> {
    if legacy_name(name).is_some() {
        return Vec::new();
    }
    let names = face_names(b);
    let stem = strip_piece(name);
    let aliases = |i: usize| naming::names(b).info.get(i).is_some_and(|x| x.synonyms.iter().chain(&x.aliases).any(|a| strip_piece(a) == stem));
    (0..names.len()).filter(|i| names.get(*i).is_some_and(|n| strip_piece(n) == stem) || aliases(*i)).collect()
}

/// [`locate_face`] on one body.
pub(crate) fn locate_on(body: &ModelBody, name: &str, p: Vec3) -> Option<FaceHit> {
    locate_in(std::iter::once((0, body)), name, Some(p)).ok()
}

/// Find a saved manual Project face in its body. Older designs can name a numbered piece of
/// a face that now evaluates whole. Recover it only when the original feature directly made
/// the exact whole face and the face contains the
/// old pick. [`locate_face`] does the same for the other stored face references (#13252).
pub(crate) fn locate_projected_face(body: &ModelBody, name: &str, p: Vec3) -> Option<(usize, Vec3)> {
    if body.body.is_mesh() {
        return None;
    }
    match locate_in(std::iter::once((0, body)), name, Some(p)) {
        Ok(hit) => Some((hit.face, hit.point)),
        Err(Miss::Gone) => recover_numbered_projected_face(body, name, p),
        Err(Miss::Several) => None,
    }
}

/// A thread's saved face the body no longer has, recovered as the one round face it is now: a
/// numbered piece of a round side ([`recover_numbered_round_face`]) or a round wall under an old
/// index ([`recover_renumbered_round_face`]).
fn recover_round_face(body: &ModelBody, name: &str, pick: Vec3) -> Option<(usize, Vec3)> {
    recover_numbered_round_face(body, name, pick).or_else(|| recover_renumbered_round_face(body, name, pick))
}

/// Bound the quadratic scan in remove_faces, which takes the list of faces to remove.
const MAX_PROJECTION_RECOVERY_FACES: usize = 1024;

fn recover_numbered_projected_face(body: &ModelBody, name: &str, pick: Vec3) -> Option<(usize, Vec3)> {
    let (face, stem) = numbered_whole_face(body, name, pick)?;
    // A changed body may keep the same face name while the face absorbs a smaller piece.
    // Projecting that larger face could silently grow a downstream feature's material.
    let producer = stem.strip_prefix('F')?.split_once(':')?.0.parse::<u64>().ok()?;
    let live_construction = body.names.origin.as_ref().is_some_and(|origin| {
        producer == origin.feature.id
            && matches!(
                &origin.feature.kind,
                FeatureKind::Box { operation: Operation::NewBody, .. } | FeatureKind::Extrude { operation: Operation::NewBody, .. }
            )
            && origin.before.body(&body.name).is_none()
    });
    if producer != body.feature || !(live_construction || body.names.cached_construction == Some(producer)) {
        return None;
    }
    on_face(body, face, pick)
}

/// A thread's numbered piece of a round side (`F1:side#2`) found as the whole side: it is the one
/// face of its stem, a cylinder or cone, and the saved pick lies on it (#16602). A thread wants
/// the whole face whatever piece it was saved on; no other feature may use this.
fn recover_numbered_round_face(body: &ModelBody, name: &str, pick: Vec3) -> Option<(usize, Vec3)> {
    let (face, _) = numbered_whole_face(body, name, pick)?;
    let record = body.body.record().faces.get(face)?;
    let round = matches!(record.surface, solidcore_record::brep::Surface::Cylinder { .. } | solidcore_record::brep::Surface::Cone { .. });
    if !round {
        return None;
    }
    on_face(body, face, pick)
}

/// The one verified face a numbered name's stem finds on `body`, and that stem.
fn numbered_whole_face<'n>(body: &ModelBody, name: &'n str, pick: Vec3) -> Option<(usize, &'n str)> {
    let (stem, number) = name.rsplit_once('#')?;
    let count = body.body.face_count();
    if count > MAX_PROJECTION_RECOVERY_FACES
        || !pick.is_finite()
        || stem.is_empty()
        || strip_piece(stem) != stem
        || number.is_empty()
        || number.starts_with('0')
        || !number.bytes().all(|digit| digit.is_ascii_digit())
    {
        return None;
    }
    let candidates = faces_with_stem(body, stem);
    let &[face] = candidates.as_slice() else { return None };
    let names = naming::names(body);
    let info = names.info.get(face)?;
    if names.faces.get(face)?.as_str() != stem || !info.verified || info.aliases.iter().any(|alias| strip_piece(alias) != stem) {
        return None;
    }
    Some((face, stem))
}

/// A thread's hole or tool round wall saved under a later index than it has now: 0.30.1 built a
/// round tool in several lateral pieces (`F4:bore:side:1`) where the kernel now builds one
/// (`F4:bore:side:0`). The wall is found again when it is the only face of that stem, a
/// cylinder or cone, and the saved pick lies on it (#16602). `None` for any other name.
fn recover_renumbered_round_face(body: &ModelBody, name: &str, pick: Vec3) -> Option<(usize, Vec3)> {
    let (stem, index) = name.rsplit_once(':')?;
    if body.body.is_mesh()
        || body.body.face_count() > MAX_PROJECTION_RECOVERY_FACES
        || !pick.is_finite()
        || !stem.ends_with(":side")
        || index.is_empty()
        || !index.bytes().all(|digit| digit.is_ascii_digit())
        || strip_piece(name) != name
    {
        return None;
    }
    let names = naming::names(body);
    let mut found = None;
    for (face, face_name) in names.faces.iter().enumerate() {
        // Every face of the stem counts, split pieces (`:1#2`) included: only a stem with one
        // face left can stand for the missing index.
        let Some((other, _)) = face_name.rsplit_once(':') else { continue };
        if other != stem {
            continue;
        }
        if found.replace(face).is_some() {
            return None;
        }
    }
    let face = found?;
    let record = body.body.record().faces.get(face)?;
    let round = matches!(record.surface, solidcore_record::brep::Surface::Cylinder { .. } | solidcore_record::brep::Surface::Cone { .. });
    (round && names.info.get(face)?.verified).then(|| on_face(body, face, pick)).flatten()
}

/// The point of face `face` nearest `pick`, when `pick` lies on that face itself (the exact
/// query is the kernel's [`solidcraft_kernel::Body::point_lies_on_face`]).
fn on_face(body: &ModelBody, face: usize, pick: Vec3) -> Option<(usize, Vec3)> {
    let tolerance = (body.body.size() * 1e-3).max(1e-3);
    let point = body.body.point_lies_on_face(face, pick, tolerance).ok()??;
    Some((face, point))
}

/// How close, relative to a body's size, a saved pick must lie to a face for
/// [`faces_at_saved_point`]: a pick is a point the face had, so only rounding separates them.
const SAVED_POINT_TOL: f64 = 1e-7;

/// Whether a stored face name that finds nothing may be found again at its saved point
/// ([`faces_at_saved_point`]): the feature and role that made the named face still make a face
/// of one of `bodies` (the name's root, before its first `@` or `#`: `F73:side:l47` of
/// `F73:side:l47@F90.1@F174.1`), so only how the face was told apart changed, as when its name
/// was worked out from geometry before Align kept history (#20308, #22919). A face whose own
/// root is gone (its sketch curve deleted and drawn again) is still lost, and so are a name
/// saved before format 14 ([`LEGACY_MARK`]), whose faces were cut differently, and a sheet's
/// face name.
pub(crate) fn refound_at_saved_point<'b>(name: &str, bodies: impl IntoIterator<Item = &'b ModelBody>) -> bool {
    if name.is_empty() || legacy_name(name).is_some() || crate::sheet::split_sheet_face_name(name).is_some() {
        return false;
    }
    let root = name_root(name);
    if root.is_empty() {
        return false;
    }
    bodies.into_iter().filter(|b| !b.body.is_mesh()).any(|b| face_names(b).iter().any(|own| name_root(own) == root))
}

/// A face name up to its first `@` (a copy or a move) or `#` (a piece) outside brackets.
fn name_root(name: &str) -> &str {
    let mut depth = 0usize;
    for (i, c) in name.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            '@' | '#' if depth == 0 => return &name[..i],
            _ => {}
        }
    }
    name
}

/// The faces of `body` that a saved pick `p` lies on, to within a ten-millionth of the body's
/// size (at least a micrometre), each with the point on it. A stored face name can find nothing
/// although the face is still there, unchanged: the name was worked out from geometry when it
/// was saved (an Align that kept no history, #20308) and the face has its exact name now
/// (#22919). The faces under the pick are only candidates: the caller takes one only when what
/// it reads from it is exactly what it read before (the same plane, the same projected edges).
pub(crate) fn faces_at_saved_point(body: &ModelBody, p: Vec3) -> Vec<(usize, Vec3)> {
    if body.body.is_mesh() || !p.is_finite() || body.body.face_count() > MAX_PROJECTION_RECOVERY_FACES {
        return Vec::new();
    }
    let tolerance = (body.body.size() * SAVED_POINT_TOL).max(1e-3 * 1e-3);
    (0..body.body.face_count()).filter_map(|face| Some((face, body.body.point_lies_on_face(face, p, tolerance).ok()??))).collect()
}

/// The one face a name `model.faces` lists finds, on the body called `body` or, without one, on
/// any body. `Err` says why there is no such face, or why the name does not name just one (the
/// pieces of a split face, or the same face on several bodies): a command that takes a face by
/// its name has no point to tell the pieces apart by.
pub fn locate_one(st: &ModelState, body: Option<&str>, name: &str) -> Result<FaceHit, String> {
    if let Some(n) = body
        && !st.bodies.iter().any(|b| b.name == n)
    {
        return Err(format!("no body `{n}`"));
    }
    if let Some((sheet_body, _)) = crate::sheet::split_sheet_face_name(name) {
        if body.is_some_and(|n| n != sheet_body) {
            return Err(format!("`{name}` is a face of `{sheet_body}`"));
        }
        // No sheet names a face so now (see [`crate::sheet::sheet_face_name`]).
        return Err(format!("no face is named `{name}` (`model.faces` lists the names)"));
    }
    let bodies: Vec<(usize, &ModelBody)> =
        st.bodies.iter().enumerate().filter(|(_, b)| !b.body.is_mesh() && body.is_none_or(|n| b.name == n)).collect();
    // As [`locate_face`]: the names of several bodies are worked out on all cores first.
    if bodies.len() > 1 && legacy_name(name).is_none() {
        naming::warm_names(bodies.iter().map(|(_, b)| *b));
    }
    locate_in(bodies.into_iter(), name, None).map_err(|miss| match miss {
        Miss::Gone => format!("no face is named `{name}` (`model.faces` lists the names)"),
        Miss::Several => {
            format!("`{name}` names more than one face (pieces of a split face, or faces of several bodies): give `body`, or the name of one piece")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naming::{NameCell, edge_name_index, vertex_name_index};

    fn blend_named_box(old_form_on_body: bool) -> ModelBody {
        let solid = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 6., 4.)).unwrap();
        let mut body = ModelBody::new("BlendBox".into(), solid, 1);
        let blend = if old_form_on_body { "F5:blend:F2:side:l1|F2:top" } else { "F5:blend(F2:side:l1|F2:top)" };
        let faces = vec![blend.into(), "F2:side:l1".into(), "F2:side:l2".into(), "F2:side:l3".into(), "F2:side:l4".into(), "F2:top".into()];
        body.names = NameCell::known(Arc::new(faces));
        body
    }

    #[test]
    fn locate_edge_and_body_edges_hit_cache_and_preserve_old_form_and_retied_lookup() {
        let body = blend_named_box(false);
        let idx = edge_name_index(&body);
        let be = BodyEdges::new(&body);
        assert!(Arc::ptr_eq(&be.edges, &idx), "BodyEdges must share cached EdgeNameIndex");

        let (i, modern_name) = idx.names.iter().enumerate().find(|(_, n)| n.contains("F5:blend(F2:side:l1|F2:top)")).unwrap();
        assert_eq!(locate_edge(&body, modern_name, Vec3::ZERO).unwrap().edge, i);
        assert!(Arc::ptr_eq(&edge_name_index(&body), &idx));
        assert!(matches!(be.matching(modern_name), Some(EdgeMatch::Edge(hit)) if hit == i));

        // Older colon-nested query finds the same edge in O(1) via canonicalized lookup.
        let old_query = modern_name.replace("F5:blend(F2:side:l1|F2:top)", "F5:blend:F2:side:l1|F2:top");
        assert_ne!(old_query, *modern_name);
        assert_eq!(locate_edge(&body, &old_query, Vec3::ZERO).unwrap().edge, i);

        // A body whose stored face name itself uses the older form still matches modern queries.
        let legacy_body = blend_named_box(true);
        let legacy_idx = edge_name_index(&legacy_body);
        let (li, legacy_edge) = legacy_idx.names.iter().enumerate().find(|(_, n)| n.contains("F5:blend:F2:side:l1|F2:top")).unwrap();
        let modern_query = legacy_edge.replace("F5:blend:F2:side:l1|F2:top", "F5:blend(F2:side:l1|F2:top)");
        assert_eq!(locate_edge(&legacy_body, &modern_query, Vec3::ZERO).unwrap().edge, li);
        assert!(locate_edge(&body, "?F2:side:l1|F2:top", Vec3::ZERO).is_err());

        // `retied_edge` resolves an opposite tie-numbered cap edge using the cached EdgeNameIndex.
        let solid = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 6., 4.)).unwrap();
        let face_infos = solid.faces(0.01).unwrap();
        let mut names = vec![String::new(); face_infos.len()];
        let mut side = 1usize;
        for f in &face_infos {
            let n = f.plane_normal.unwrap();
            names[f.index] = if n.z > 0.9 {
                "F2:top#1".into()
            } else if n.z < -0.9 {
                "F2:top#2".into()
            } else {
                let s = format!("F2:side:l{side}");
                side += 1;
                s
            };
        }
        let mut retied_body = ModelBody::new("Retied".into(), solid, 1);
        retied_body.names = NameCell::known(Arc::new(names));
        let m = retied_body.mesh();
        let ridx = edge_name_index(&retied_body);
        let e1 = ridx.names.iter().position(|n| n == "F2:side:l1|F2:top#1").unwrap();
        let e2 = ridx.names.iter().position(|n| n == "F2:side:l1|F2:top#2").unwrap();
        let mid2 = edge_mid(&m, e2).unwrap();
        assert_eq!(retied_edge(&retied_body, &m, &ridx.names[e1], e1, mid2), Some(e2));
        assert!(Arc::ptr_eq(&edge_name_index(&retied_body), &ridx));
    }

    #[test]
    fn locate_vertex_hits_cache_and_handles_old_form_and_errors() {
        let body = blend_named_box(false);
        let idx = vertex_name_index(&body);
        let (v, modern_name) = idx.names.iter().enumerate().find(|(_, n)| n.contains("F5:blend(F2:side:l1|F2:top)")).unwrap();
        assert_eq!(locate_vertex(&body, modern_name, Vec3::ZERO).unwrap().vertex, v);
        assert!(Arc::ptr_eq(&vertex_name_index(&body), &idx));

        let old_query = modern_name.replace("F5:blend(F2:side:l1|F2:top)", "F5:blend:F2:side:l1|F2:top");
        assert_eq!(locate_vertex(&body, &old_query, Vec3::ZERO).unwrap().vertex, v);

        let legacy_body = blend_named_box(true);
        let legacy_idx = vertex_name_index(&legacy_body);
        let (lv, legacy_vertex) = legacy_idx.names.iter().enumerate().find(|(_, n)| n.contains("F5:blend:F2:side:l1|F2:top")).unwrap();
        let modern_query = legacy_vertex.replace("F5:blend:F2:side:l1|F2:top", "F5:blend(F2:side:l1|F2:top)");
        assert_eq!(locate_vertex(&legacy_body, &modern_query, Vec3::ZERO).unwrap().vertex, lv);

        assert!(locate_vertex(&body, "", Vec3::ZERO).is_err());
        assert!(locate_vertex(&body, "?F2:side:l1|F2:top", Vec3::ZERO).is_err());

        let mesh_body = ModelBody::new(
            "Mesh".into(),
            solidcraft_kernel::mesh_body(&[Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z], &[[0, 1, 2], [0, 1, 3], [1, 2, 3], [0, 2, 3]]).unwrap(),
            1,
        );
        assert!(locate_vertex(&mesh_body, modern_name, Vec3::ZERO).is_err());
    }
}
