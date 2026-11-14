//! Finding a face or edge again by the name a feature stored for it.
//!
//! The face with that name is the face (a face its feature made from several of its roles, a
//! side drawn in collinear pieces, has each as a name); else the one face answering to it as
//! exactly: the named piece told apart by the faces around it, which still bound it. Nothing
//! else is: when the named face was split into several pieces, when it was merged into another
//! face, when only a face of the same stem is left, when the name is on several bodies, or when
//! the name was saved before format 14 ([`LEGACY_MARK`]), the name finds nothing and the feature
//! that uses it fails, asking for the reference to be picked again
//! ([`crate::DocError::Reselect`]). An edge name has two more ways: when no edge's faces answer
//! to it, and one of its faces is gone from a feature still on the body, the reference's point
//! decides (see [`renamed_edge`]); so it does when a face of it is named after neighbours it no
//! longer has, or answers to several faces, and the point lies on one edge between them (see
//! [`resuffixed_edge`]). A face whose name was worked out from geometry, found where
//! the reference's point no longer lies, is reported: only geometry decided.
//! Saved numbered Project links also have a compatibility check for a whole face that
//! contains the saved pick and comes directly from its original construction feature.
//! This check is local to manual sketch projection. A thread alone also finds a round side
//! again that 0.30.1 saved as a numbered piece (`F2:side#2`) or under a later index
//! (`F4:bore:side:1`): see [`locate_thread_face_on_body`] (#16602).

use solidcore_record::Vec3;
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;

use crate::history_names::{Match, Names};
use crate::naming::{self, closest, edge_names, face_names, strip_piece};
use crate::piece_names::strip_edge_number;
use crate::{FeatureKind, ModelBody, ModelState, Operation};

/// The mark on a name saved before format 14 (`?F2:top#2`). Those names were worked out from
/// geometry, and some of them name a face that exact history names differently (a tube's outer
/// wall was named after the nearest sketch curve to its centre, its inner circle): such a name
/// finds no face, and the feature using it asks for the face to be picked again, which stores
/// its exact name.
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
    let names = edge_names(b);
    let faces = naming::names(b);
    let mid = |i: usize| edge_mid(&m, i);
    // Several edges between the same two faces are numbered by where they lie: not exact.
    let verified = |i: usize| {
        m.edge_faces.get(i).is_some_and(|fs| fs.iter().all(|f| faces.verified(*f as usize)))
            && names.get(i).is_some_and(|n| strip_edge_number(n).len() == n.len())
    };
    let exact = |i: usize| -> Result<EdgeHit, String> {
        let on = edge_near(b, &m, i, near);
        mid(i).map(|p| EdgeHit { edge: i, point: if on { near } else { p }, verified: verified(i), on_point: on }).ok_or_else(|| "edge".into())
    };
    if let Some(i) = names.iter().position(|n| crate::persistent_name::same_edge(n, name)) {
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

/// Find a saved Sketch Project edge in its body. Older designs can name the edge between
/// numbered pieces of faces that now evaluate whole (`F4:side:l6#2|F4:side:l7#2`, now
/// `F4:side:l6|F4:side:l7`). Recover it only when the faces it separates are whole, the saved
/// point lies on it, and its name is exact. Other edge references still require the stored
/// name as picked.
pub(crate) fn locate_projected_edge(b: &ModelBody, name: &str, near: Vec3) -> Result<EdgeHit, String> {
    locate_edge(b, name, near).or_else(|miss| recover_numbered_projected_edge(b, name, near).ok_or(miss))
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
    let incident = naming::vertex_incident_faces(record);
    let faces = naming::names(b);
    let names = naming::vertex_names_from_record(record, &incident, &faces.faces);
    let reach = pick_reach(b);
    let pos = |v: usize| record.vertices.get(v).map(|v| Vec3::new(v.point[0], v.point[1], v.point[2])).filter(|p| p.is_finite());
    let verified = |v: usize| {
        incident.get(v).is_some_and(|fs| !fs.is_empty() && fs.iter().all(|f| faces.verified(*f as usize)))
            && names.get(v).is_some_and(|n| strip_edge_number(n).len() == n.len())
    };
    let hit = |v: usize| -> Result<VertexHit, String> {
        let point = pos(v).ok_or_else(|| "vertex".to_string())?;
        let on_point = near.is_finite() && point.dist(near) <= reach;
        Ok(VertexHit { vertex: v, point, verified: verified(v), on_point })
    };
    if let Some(v) = names
        .iter()
        .enumerate()
        .find(|(v, n)| !n.is_empty() && incident.get(*v).is_some_and(|fs| !fs.is_empty()) && crate::persistent_name::same_edge(n, name))
        .map(|(v, _)| v)
    {
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
            resuffixed_vertex(record, &incident, &faces, &wanted_parsed, reach, near)
        } else {
            renamed_vertex(record, &incident, &faces, &wanted, reach, near)
        };
        return picked.and_then(at_pick).ok_or_else(|| format!("vertex `{name}` no longer exists"));
    };
    let vertex_near = |v: usize| pos(v).is_some_and(|p| near.is_finite() && p.dist(near) <= reach);
    match cands.as_slice() {
        [v] if numbered && !vertex_near(*v) => Err(format!("vertex `{name}` was one of several between its faces, and the one left is elsewhere")),
        [v] => hit(*v),
        _ => resuffixed_vertex(record, &incident, &faces, &wanted_parsed, reach, near)
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
        (level == Match::Stem && qualified(x) && faces.faces.get(f).is_some_and(|n| qualified(n))).then_some(true)
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
fn pick_reach(b: &ModelBody) -> f64 {
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
fn renamed_edge(b: &ModelBody, m: &Mesh, faces: &Names, wanted: &str, near: Vec3) -> Option<usize> {
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
    // Every way the name splits into two face names (a face name may hold `|` itself); they must
    // agree on the face still found.
    let mut kept: Vec<(usize, u64)> = Vec::new();
    for (i, _) in wanted.match_indices('|') {
        let (Some(x), Some(y)) = (wanted.get(..i), wanted.get(i + 1..)) else { continue };
        for (x, y) in [(x, y), (y, x)] {
            if let Some(hit) = found(x).filter(|f| kept.iter().all(|(k, _)| k != f)).zip(lost(y)) {
                kept.push(hit);
            }
        }
    }
    let &[(face, lost_by)] = kept.as_slice() else { return None };
    let edge = edge_through(b, m, near)?;
    let uses = &b.body.record().edges.get(edge)?.uses;
    // The edge's other face is not one of the lost face's feature: where that feature made the
    // face at the pick, its face was replaced there (a sketch line swapped for another), not renamed.
    let made_by = |f: usize| faces.faces.get(f).and_then(|name| crate::persistent_name::made_by(name));
    let bounds_face = uses.iter().any(|u| u.face as usize == face);
    let replaced = uses.iter().any(|u| u.face as usize != face && made_by(u.face as usize) == Some(lost_by));
    (bounds_face && !replaced).then_some(edge)
}

/// The edge a stored edge name `wanted` (without its number) meant when one of its faces is named
/// with the faces around it (`F44:top#[F43:side:l7]`) and those are now others: the face is still
/// there under its stem, but its qualifier changed, or several faces answer to it (a join over
/// another extrude's footprint names the walls it joins differently, and can leave two faces of
/// that stem). The candidates are the edges between faces answering to the name's two faces: each
/// exactly (see [`exactly`]) or, for a face named with its qualifier, by its stem where that face
/// too is named with a qualifier (another one: a face whole again is not it), at least one
/// side of them by a qualifier that is not that face's own (a piece of the named face split since
/// is not a renamed face: the edge split with it and the name asks again). Of those, the one edge
/// whose exact curve the reference's point `near` lies on (within the pick reach, as
/// [`solidcraft_kernel::edge_distance`] measures it) is taken; none or several, and the name finds
/// nothing. The point must be the reference's own pick, as for [`renamed_edge`].
fn resuffixed_edge(b: &ModelBody, m: &Mesh, faces: &Names, wanted: &str, near: Vec3) -> Option<usize> {
    if b.body.is_mesh() || !near.is_finite() {
        return None;
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
        (level == Match::Stem && qualified(x) && faces.faces.get(f).is_some_and(|n| qualified(n))).then_some(true)
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
    /// either side of it), `None` when the line misses the face or runs along it. A place on
    /// a face follows the face this way, along its own axis, instead of sliding sideways to
    /// [`point`](Self::point), the face's nearest point.
    pub fn along(&self, st: &ModelState, p: Vec3, dir: Vec3) -> Option<Vec3> {
        let b = st.bodies.get(self.body)?;
        let d = dir.normalized()?;
        let m = b.mesh();
        let slack = (b.body.size() * 1e-9).max(1e-9);
        let mut best: Option<f64> = None;
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            if *f as usize != self.face {
                continue;
            }
            let Some([a, bb, c]) = m.tri(t) else { continue };
            // A hair's width around each triangle, so a place on the seam of two is on the face.
            let centre = (a + bb + c) * (1.0 / 3.0);
            let grow = |v: Vec3| v + (v - centre).normalized().unwrap_or(Vec3::ZERO) * slack;
            if let Some(hit) = naming::ray_hit(p, d, grow(a), grow(bb), grow(c))
                && best.is_none_or(|old| hit.abs() < old.abs())
            {
                best = Some(hit);
            }
        }
        best.map(|t| p + d * t)
    }

    /// A point that picks the face found again by itself: [`point`](Self::point) when the
    /// reference's point still lies on the face clear of its edges; else the point of the face
    /// nearest `p` that keeps clear of every edge of it. The nearest point of a face the
    /// reference's point no longer lies on is on the face's boundary (a parameter moved the face
    /// away from the pick), where a pick by point finds the face beside it as well (#2531).
    pub fn pick_point(&self, st: &ModelState, p: Vec3) -> Vec3 {
        let Some(b) = st.bodies.get(self.body) else { return self.point };
        let m = b.mesh();
        // Clear of the edges by a few pick tolerances, or by half the face's roomiest triangle
        // on a face narrower than that.
        let Some(margin) = roomiest_inradius(&m, self.face).map(|r| (4.0 * (b.body.size() * 1e-3).max(1e-3)).min(r * 0.5)) else {
            return self.point;
        };
        if self.on_point && edge_clearance(&m, self.face, self.point) >= margin {
            return self.point;
        }
        inside_nearest(&m, self.face, p, margin).unwrap_or(self.point)
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
    if crate::sheet::split_sheet_face_name(name).is_some() {
        return sheet_locate(st, name, p).ok();
    }
    if legacy_name(name).is_some() {
        return None;
    }
    // Every body's names are read below: work out the ones not known yet on all cores first
    // (each body's names are its own, whichever thread works them out).
    naming::warm_names(st.bodies.iter().filter(|b| !b.body.is_mesh()));
    match locate_in(st.bodies.iter().enumerate().filter(|(_, b)| !b.body.is_mesh()), name, p) {
        Ok(hit) => Some(hit),
        Err(Miss::Gone) => recover_numbered_face_in(st, name, p, round),
        Err(Miss::Several) => None,
    }
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
    let mut holders = solids().filter(|b| {
        let names = naming::names(b);
        (0..names.faces.len()).any(|face| crate::history_names::matches(&names, face, name).is_some_and(exactly))
    });
    match (holders.next(), holders.next()) {
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

fn locate_face_on_body_with(st: &ModelState, body: Option<&str>, name: &str, p: Vec3, round: bool) -> Option<FaceHit> {
    if crate::sheet::split_sheet_face_name(name).is_some() || legacy_name(name).is_some() {
        return locate_face_with(st, name, p, round);
    }
    if let Some(i) = body.and_then(|n| st.bodies.iter().position(|b| b.name == n))
        && let Some(b) = st.bodies.get(i).filter(|b| !b.body.is_mesh())
    {
        match locate_in(std::iter::once((i, b)), name, p) {
            Ok(hit) => return Some(hit),
            Err(Miss::Several) => return None,
            Err(Miss::Gone) => {}
        }
    }
    locate_face_with(st, name, p, round)
}

/// [`locate_face`] for a sheet face's readable name (`sheet:<body>:<panel>:<side>`, see
/// [`crate::sheet::body_sheet_face_names`]): the face of that body the sheet names so. The sheet
/// itself says which face it is, so the name holds however the faces of the body are numbered.
fn sheet_locate(st: &ModelState, name: &str, p: Vec3) -> Result<FaceHit, Miss> {
    let (body, _) = crate::sheet::split_sheet_face_name(name).ok_or(Miss::Gone)?;
    let (bi, b) = st.bodies.iter().enumerate().find(|(_, b)| b.name == body && !b.body.is_mesh()).ok_or(Miss::Gone)?;
    let names = crate::sheet::body_sheet_face_names(st, b);
    let faces: Vec<usize> = names.iter().enumerate().filter(|(_, n)| n.as_deref() == Some(name)).map(|(i, _)| i).collect();
    let face = match faces.as_slice() {
        [face] => *face,
        [] => return Err(Miss::Gone),
        _ => return Err(Miss::Several),
    };
    let (distance, point) = nearest_on_face(&b.mesh(), face, p).ok_or(Miss::Gone)?;
    Ok(FaceHit { body: bi, face, point, verified: true, on_point: distance <= (b.body.size() * 1e-3).max(1e-3) })
}

/// [`locate_face`] among `bodies` (with their indices): the one face the name finds there.
fn locate_in<'a>(bodies: impl Iterator<Item = (usize, &'a ModelBody)>, name: &str, p: Vec3) -> Result<FaceHit, Miss> {
    if legacy_name(name).is_some() {
        return Err(Miss::Gone);
    }
    let mut best: Option<(Match, Vec<(usize, &ModelBody, usize)>)> = None;
    for (bi, b) in bodies {
        let names = naming::names(b);
        for face in 0..names.faces.len() {
            if let Some(level) = crate::history_names::matches(&names, face, name).filter(|level| exactly(*level)) {
                keep_best(&mut best, level, (bi, b, face));
            }
        }
    }
    let (level, faces) = best.ok_or(Miss::Gone)?;
    let found = |(body, b, face): (usize, &ModelBody, usize)| {
        let (distance, point) = nearest_on_face(&b.mesh(), face, p)?;
        Some(FaceHit { body, face, point, verified: naming::names(b).verified(face), on_point: distance <= (b.body.size() * 1e-3).max(1e-3) })
    };
    if let [one] = faces.as_slice() {
        return found(*one).ok_or(Miss::Gone);
    }
    // The very face on several bodies (a body and its copies, whose faces keep their names): the
    // one the reference's point lies on. Pieces of one face are never told apart by a point.
    let mut bodies: Vec<usize> = faces.iter().map(|(bi, _, _)| *bi).collect();
    bodies.dedup();
    if level != Match::Exact || bodies.len() < faces.len() {
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
    legacy_name(name).is_none() && crate::history_names::matches(&naming::names(b), index, name).is_some_and(exactly)
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
    locate_in(std::iter::once((0, body)), name, p).ok()
}

/// Find a saved manual Project face in its body. Older designs can name a numbered piece of
/// a face that now evaluates whole. Recover it only when the original feature directly made
/// the exact whole face and the face contains the
/// old pick. [`locate_face`] does the same for the other stored face references (#13252).
pub(crate) fn locate_projected_face(body: &ModelBody, name: &str, p: Vec3) -> Option<(usize, Vec3)> {
    if body.body.is_mesh() {
        return None;
    }
    match locate_in(std::iter::once((0, body)), name, p) {
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
        return sheet_locate(st, name, Vec3::ZERO).map_err(|miss| match miss {
            Miss::Gone => format!("no face is named `{name}` (`model.faces` lists the names)"),
            Miss::Several => format!("`{name}` names more than one face (pieces of a split face): give the name of one piece"),
        });
    }
    let bodies: Vec<(usize, &ModelBody)> =
        st.bodies.iter().enumerate().filter(|(_, b)| !b.body.is_mesh() && body.is_none_or(|n| b.name == n)).collect();
    // As [`locate_face`]: the names of several bodies are worked out on all cores first.
    if bodies.len() > 1 && legacy_name(name).is_none() {
        naming::warm_names(bodies.iter().map(|(_, b)| *b));
    }
    locate_in(bodies.into_iter(), name, Vec3::ZERO).map_err(|miss| match miss {
        Miss::Gone => format!("no face is named `{name}` (`model.faces` lists the names)"),
        Miss::Several => {
            format!("`{name}` names more than one face (pieces of a split face, or faces of several bodies): give `body`, or the name of one piece")
        }
    })
}
