//! Finding a face or edge again by the name a feature stored for it.
//!
//! The face with that name is the face (a face its feature made from several of its roles, a
//! side drawn in collinear pieces, has each as a name); else the one face answering to it as
//! exactly: the named piece told apart by the faces around it, which still bound it. Nothing
//! else is: when the named face was split into several pieces, when it was merged into another
//! face, when only a face of the same stem is left, when the name is on several bodies, or when
//! the name was saved before format 14 ([`LEGACY_MARK`]), the name finds nothing and the feature
//! that uses it fails, asking for the reference to be picked again
//! ([`crate::DocError::Reselect`]). A face whose name was worked out from geometry, found where
//! the reference's point no longer lies, is reported: only geometry decided.

use solidcraft_geom::{Mesh, Vec3};

use crate::history_names::{Match, Names};
use crate::naming::{self, closest, edge_names, face_names, strip_piece};
use crate::piece_names::strip_edge_number;
use crate::{ModelBody, ModelState};

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
/// between the faces it names exactly (see [`exactly`]). `Err` explains when there is none: no
/// such edge, the named edge split into several, or a name saved before format 14.
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
    if let Some(i) = names.iter().position(|n| n == name) {
        return exact(i);
    }
    // By the faces it separates: the best way any edge matches.
    let wanted = strip_edge_number(name);
    let mut best: Option<(Match, Vec<usize>)> = None;
    for (i, fs) in m.edge_faces.iter().enumerate() {
        let [fa, fb] = fs.as_slice() else { continue };
        let Some(level) = edge_match(&faces, *fa as usize, *fb as usize, wanted).filter(|level| exactly(*level)) else { continue };
        keep_best(&mut best, level, i);
    }
    let (_, cands) = best.ok_or_else(|| format!("edge `{name}` no longer exists"))?;
    match cands.as_slice() {
        [i] => exact(*i),
        _ => Err(format!("edge `{name}` is no longer one edge")),
    }
}

/// Whether a face found at `level` is exactly the face a name named: the very name (or another
/// name of that very face: a face its feature made from several roles has each), or the named
/// piece told apart by the faces around it. A face the named one was merged into, or a face of
/// the same stem, is not.
fn exactly(level: Match) -> bool {
    matches!(level, Match::Exact | Match::Piece)
}

/// Whether the picked point `near` lies on edge `i` of body `b` (whose mesh is `m`): then a
/// reference keeps it.
fn edge_near(b: &ModelBody, m: &Mesh, i: usize, near: Vec3) -> bool {
    m.edges.get(i).is_some_and(|e| e.windows(2).any(|w| near.dist_to_segment(w[0], w[1]) < (b.body.size() * 2e-3).max(1e-3)))
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
        if !self.verified && !self.on_point { Some(format!("face `{name}` was found by geometry, not exact history; check it")) } else { None }
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

impl EdgeHit {
    /// What a feature says about how it found an edge by its stored name `name` (`None`: exactly).
    pub fn note(&self, name: &str) -> Option<String> {
        if !self.verified && !self.on_point { Some(format!("edge `{name}` was found by geometry, not exact history; check it")) } else { None }
    }
}

/// The face a stored face name finds, on any body: the face with that name, or the named piece
/// told apart by the faces around it. `None` when no face does, or several do.
pub fn locate_face(st: &ModelState, name: &str, p: Vec3) -> Option<FaceHit> {
    if crate::sheet::split_sheet_face_name(name).is_some() {
        return sheet_locate(st, name, p).ok();
    }
    if legacy_name(name).is_some() {
        return None;
    }
    // Every body's names are read below: work out the ones not known yet on all cores first
    // (each body's names are its own, whichever thread works them out).
    naming::warm_names(st.bodies.iter().filter(|b| !b.body.is_mesh()));
    locate_in(st.bodies.iter().enumerate().filter(|(_, b)| !b.body.is_mesh()), name, p).ok()
}

/// [`locate_face`] for a feature that works on the faces of the body called `body` (a shell, a
/// draft…): the face of that body the name finds, found without working out the names of every
/// other body (a body named from geometry is meshed for it: seconds in a large model). When
/// that body has no such face, as [`locate_face`] over every body: the name moved to another
/// body (a combine took the face). The same face on another body (a copy of this one) does not
/// make this body's face ambiguous.
pub fn locate_face_on_body(st: &ModelState, body: Option<&str>, name: &str, p: Vec3) -> Option<FaceHit> {
    if crate::sheet::split_sheet_face_name(name).is_some() || legacy_name(name).is_some() {
        return locate_face(st, name, p);
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
    locate_face(st, name, p)
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
