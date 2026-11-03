//! Face names from exact history, and how a stored name matches a face.
//!
//! A face's name says what made it (see `naming` for the vocabulary). Where the feature that made
//! a body reports its history (a [`Lineage`]), each face is named after the faces it came from:
//! kept, trimmed or moved faces keep their names, a face made from a profile segment or cap is
//! named by that role, a fillet face by the edge it replaces, a shell's inner wall by the face it
//! offsets. Nothing is sampled. Where a face's origin cannot be named exactly the face is named
//! from geometry, as a body without history is, and marked unverified.
//!
//! Two faces with the same name are pieces of one face (a cut split it). Each piece is told apart
//! by the faces around it that the others do not have: `F2:top#[F2:side:l4,F4:side:l4]` is the
//! piece of `F2:top` bounded by the block's left wall and the slot's left wall. Pieces nothing
//! around them tells apart are numbered (`#1`, `#2`): in the order of what they came from when
//! that differs (exact), else in the kernel's order (unverified). A face made of several (a
//! coplanar join) takes one name and keeps the others as aliases: a reference to one of them is
//! to a face that is no longer as it was picked, and finds nothing. A face a feature built from
//! several of its own roles (a side drawn in collinear pieces) has each of them as a name
//! (synonyms): each names that very face.
//!
//! A piece that is split again keeps the face's stem and the bounds that told the piece it
//! came from apart: its name lists the faces around it, not the qualifiers of the pieces
//! before it, so a name does not grow with every split.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};

use solidcraft_geom::Vec3;
use solidcraft_kernel::history::{FaceOrigin, FaceProv, History};

use crate::ModelBody;
use crate::lineage::{Lineage, Operand};
use crate::naming::NamingOrigin;

/// The names of a body's faces, with what finding them again needs.
#[derive(Debug, Default)]
pub struct Names {
    pub faces: Arc<Vec<String>>,
    pub(crate) info: Vec<FaceInfo>,
}

/// What a face answers to besides its name, and how sure its name is.
#[derive(Clone, Debug, Default)]
pub(crate) struct FaceInfo {
    /// Names of the other faces merged into it. A reference to one of them is to a face that is
    /// not this one as it was picked: a feature's reference by one finds nothing (`name_lookup`).
    pub aliases: Vec<String>,
    /// Other names of this very face: the roles besides its first of a face the feature that
    /// built it made from several (a side drawn in collinear pieces is one face, made by each).
    /// A reference by one of them finds the face as its name does.
    pub synonyms: Vec<String>,
    /// The stems of the faces around it: what tells it apart from other pieces of its face.
    pub bounds: BTreeSet<String>,
    /// Whether the name follows exact history (`false`: worked out from geometry, or a piece
    /// numbered because the faces around it did not tell it apart).
    pub verified: bool,
}

impl Names {
    /// Names worked out from geometry: the same names, none of them verified, pieces told apart
    /// by the faces around them (`adjacent`: pairs of faces sharing an edge), then numbered by
    /// place (`centres`, see [`qualify_at`]).
    pub(crate) fn unverified(names: Vec<String>, adjacent: &[(usize, usize)], centres: Option<&dyn Fn() -> Vec<Vec3>>) -> Names {
        let named = names
            .into_iter()
            .map(|name| Named { name: crate::naming::strip_piece(&name).to_string(), verified: false, ..Named::default() })
            .collect();
        qualify_at(named, adjacent, centres)
    }

    /// Names given as they are (a moved copy keeps its source's), taken as exact.
    pub(crate) fn given(names: Arc<Vec<String>>) -> Names {
        let info = vec![FaceInfo { verified: true, ..FaceInfo::default() }; names.len()];
        Names { faces: names, info }
    }

    /// Whether face `index`'s name follows exact history.
    pub fn verified(&self, index: usize) -> bool {
        self.info.get(index).is_some_and(|i| i.verified)
    }
}

/// A face's name before pieces are told apart.
#[derive(Clone, Debug, Default)]
struct Named {
    name: String,
    aliases: Vec<String>,
    synonyms: Vec<String>,
    verified: bool,
    /// What it came from (`None`: not known): faces with one name but different origins (an
    /// extrude's two caps both above its sketch plane) are numbered in this order.
    origins: Option<Vec<FaceOrigin>>,
}

/// Names of a body (`b`, made by `o`'s feature) from its lineage; `None` when the lineage does
/// not describe it (then it is named from geometry).
pub(crate) fn derive(b: &ModelBody, o: &NamingOrigin, lineage: &Lineage) -> Option<Names> {
    let h = &lineage.history;
    if h.faces.len() != b.body.face_count() || h.edges.len() != b.body.edge_count() {
        return None;
    }
    let operands: Vec<OperandNames> = lineage.operands.iter().map(|op| OperandNames::new(op, o)).collect::<Option<_>>()?;
    let fid = o.feature.id;
    let mut named: Vec<Option<Named>> = h.faces.iter().map(|f| from_origins(f, &operands, fid)).collect();
    if named.iter().any(Option::is_none) {
        let geometric = crate::naming::geometric_names(b, o, named.len());
        for (slot, name) in named.iter_mut().zip(geometric) {
            if slot.is_none() {
                *slot = Some(Named { name: crate::naming::strip_piece(&name).to_string(), verified: false, ..Named::default() });
            }
        }
    }
    let named: Vec<Named> = named.into_iter().collect::<Option<_>>()?;
    let centres = || crate::naming::face_centres(b);
    Some(qualify_at(named, &adjacency(h), by_place(&o.feature, &centres)))
}

/// Pairs of faces that share an edge, from a history.
fn adjacency(h: &History) -> Vec<(usize, usize)> {
    face_pairs(h.edges.iter().map(|e| e.faces.iter().copied()))
}

/// The two faces of each edge (edges given by their faces) that has exactly two.
pub(crate) fn face_pairs<I: Iterator<Item = usize>>(edges: impl Iterator<Item = I>) -> Vec<(usize, usize)> {
    edges
        .filter_map(|mut faces| match (faces.next(), faces.next(), faces.next()) {
            (Some(a), Some(b), None) => Some((a, b)),
            _ => None,
        })
        .collect()
}

/// An operand's faces' names.
struct OperandNames<'a> {
    operand: &'a Operand,
    /// The model body (or a stand-in for a stage) the operand's names belong to.
    body: Option<ModelBody>,
    names: Arc<Names>,
    /// Its edges' names, worked out on first use (for the faces that blend them).
    edges: OnceLock<Vec<String>>,
}

impl<'a> OperandNames<'a> {
    fn new(operand: &'a Operand, o: &NamingOrigin) -> Option<OperandNames<'a>> {
        match operand {
            Operand::Body(name) | Operand::Copy { body: name, .. } | Operand::Inner(name) => {
                let body = o.before.body(name)?;
                Some(OperandNames { operand, names: crate::naming::names(body), body: Some(body.clone()), edges: OnceLock::new() })
            }
            Operand::Made(_) | Operand::CopiedTool { .. } => {
                Some(OperandNames { operand, body: None, names: Arc::default(), edges: OnceLock::new() })
            }
            Operand::Stage(stage) => {
                let body = &stage.body;
                let names = stage
                    .names
                    .get_or_init(|| {
                        Arc::new(derive(body, o, &stage.lineage).unwrap_or_else(|| {
                            let names = crate::naming::geometric_names(body, o, body.body.face_count());
                            let centres = || crate::naming::face_centres(body);
                            Names::unverified(names, &crate::naming::mesh_adjacency(body), by_place(&o.feature, &centres))
                        }))
                    })
                    .clone();
                Some(OperandNames { operand, body: Some(body.clone()), names, edges: OnceLock::new() })
            }
        }
    }

    /// The name of face `face`.
    fn face(&self, face: usize, fid: u64) -> Option<Named> {
        let given = |names: &Names| {
            let info = names.info.get(face)?;
            Some(Named {
                name: names.faces.get(face)?.clone(),
                aliases: info.aliases.clone(),
                synonyms: info.synonyms.clone(),
                verified: info.verified,
                origins: None,
            })
        };
        match self.operand {
            Operand::Body(_) | Operand::Stage(_) => given(&self.names),
            Operand::Copy { instance, .. } => given(&self.names).map(|n| {
                let copied = |names: &[String]| names.iter().map(|a| format!("{a}@F{fid}.{instance}")).collect();
                Named {
                    name: format!("{}@F{fid}.{instance}", n.name),
                    aliases: copied(&n.aliases),
                    synonyms: copied(&n.synonyms),
                    verified: n.verified,
                    origins: None,
                }
            }),
            Operand::Inner(_) => {
                given(&self.names).map(|n| Named { name: format!("F{fid}:inner:{}", n.name), verified: n.verified, ..Named::default() })
            }
            Operand::Made(roles) => {
                let (first, rest) = roles.get(face)?.split_first()?;
                Some(Named {
                    name: format!("F{fid}:{first}"),
                    synonyms: rest.iter().map(|r| format!("F{fid}:{r}")).collect(),
                    verified: true,
                    ..Named::default()
                })
            }
            Operand::CopiedTool { roles, feature, instance } => {
                let named = |role: &String| format!("F{feature}:{role}@F{fid}.{instance}");
                let (first, rest) = roles.get(face)?.split_first()?;
                Some(Named { name: named(first), synonyms: rest.iter().map(named).collect(), verified: true, ..Named::default() })
            }
        }
    }

    /// The name of edge `edge`, and whether it is exact.
    fn edge(&self, edge: usize) -> Option<(String, bool)> {
        match self.operand {
            Operand::Body(_) | Operand::Stage(_) => {
                let body = self.body.as_ref()?;
                let names = self.edges.get_or_init(|| crate::naming::edge_names(body));
                let faces = body.mesh().edge_faces.get(edge).cloned().unwrap_or_default();
                let verified = faces.iter().all(|f| self.names.verified(*f as usize));
                Some((names.get(edge)?.clone(), verified))
            }
            Operand::Copy { .. } | Operand::Inner(_) | Operand::Made(_) | Operand::CopiedTool { .. } => None,
        }
    }
}

/// A face's name from what it came from; `None` when an origin cannot be named exactly.
fn from_origins(face: &FaceProv, operands: &[OperandNames], fid: u64) -> Option<Named> {
    let mut origins: Vec<FaceOrigin> = face.origins.clone();
    origins.sort_unstable();
    let mut names: Vec<Named> = Vec::with_capacity(origins.len());
    for origin in origins.iter().copied() {
        let named = match origin {
            FaceOrigin::Face { operand, face } => operands.get(operand)?.face(face, fid)?,
            FaceOrigin::Edge { operand, edge } => {
                let (edge, verified) = operands.get(operand)?.edge(edge)?;
                Named { name: format!("F{fid}:blend:{edge}"), verified, ..Named::default() }
            }
            FaceOrigin::Offset { operand, face } => {
                let source = operands.get(operand)?.face(face, fid)?;
                Named { name: format!("F{fid}:inner:{}", source.name), verified: source.verified, ..Named::default() }
            }
            // A profile or cap of a body made from nothing: only a built body's own roles name
            // these, and those come in as `Made` operands.
            FaceOrigin::Profile { .. } | FaceOrigin::Cap { .. } => return None,
        };
        if !names.iter().any(|n| n.name == named.name) {
            names.push(named);
        }
    }
    merged(names).map(|n| Named { origins: Some(origins), ..n })
}

/// One name for a face made of faces with these names: the face they are all pieces of, else the
/// first (the target's before a tool's); the others become aliases. The synonyms of the face
/// whose name it takes stay its synonyms.
fn merged(names: Vec<Named>) -> Option<Named> {
    let verified = names.iter().all(|n| n.verified);
    let first = names.first()?.name.clone();
    let mut common = Some(first.as_str());
    while let Some(c) = common {
        if names.iter().all(|n| descends(&n.name, c)) {
            break;
        }
        common = crate::naming::parent_piece(c);
    }
    let name = common_piece(&names).or_else(|| common.map(str::to_string)).unwrap_or(first);
    let (mut aliases, mut synonyms): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for n in names {
        let (kept, merged) = if n.name == name { (n.synonyms, n.aliases) } else { (Vec::new(), [vec![n.name], n.synonyms, n.aliases].concat()) };
        for synonym in kept {
            if synonym != name && !synonyms.contains(&synonym) {
                synonyms.push(synonym);
            }
        }
        for alias in merged {
            if alias != name && !aliases.contains(&alias) {
                aliases.push(alias);
            }
        }
    }
    Some(Named { name, aliases, synonyms, verified, origins: None })
}

/// The name the faces have in common: their stem, the faces their qualifiers share and the
/// number that told their pieces apart, when all of them have it. A face made of pieces that
/// were split again keeps the name of the piece they came from (#5860).
fn common_piece(names: &[Named]) -> Option<String> {
    let first = names.first()?.name.as_str();
    let (stem, around, tie) = split_piece(first);
    let mut shared: BTreeSet<String> = around.into_iter().collect();
    let mut same_tie = tie;
    for n in names.iter().skip(1) {
        let (other_stem, around, other_tie) = split_piece(&n.name);
        if other_stem != stem {
            return None;
        }
        let around: BTreeSet<String> = around.into_iter().collect();
        shared = shared.intersection(&around).cloned().collect();
        if same_tie != other_tie {
            same_tie = None;
        }
    }
    let mut name = stem;
    if !shared.is_empty() {
        name.push_str("#[");
        name.push_str(&shared.into_iter().collect::<Vec<_>>().join(","));
        name.push(']');
    }
    if let Some(tie) = same_tie {
        name.push('#');
        name.push_str(&tie);
    }
    Some(name)
}

/// Whether `name` is `stem` or a piece of it (at any depth).
pub(crate) fn descends(name: &str, stem: &str) -> bool {
    let mut current = Some(name);
    while let Some(n) = current {
        if n == stem {
            return true;
        }
        current = crate::piece_names::parent_piece(n);
    }
    false
}

/// Tell pieces apart: faces sharing a name are named by the faces around them that the others
/// lack (`adjacent`: pairs of faces sharing an edge).
#[cfg(test)]
fn qualify(named: Vec<Named>, adjacent: &[(usize, usize)]) -> Names {
    qualify_at(named, adjacent, None)
}

/// How the pieces `f` splits a face into are numbered: by place (`centres`), else, for a feature
/// saved before that (`Feature::kernel_piece_order`), as the build that saved it numbered them.
pub(crate) fn by_place<'a>(f: &crate::Feature, centres: &'a dyn Fn() -> Vec<Vec3>) -> Option<&'a dyn Fn() -> Vec<Vec3>> {
    (!f.kernel_piece_order).then_some(centres)
}

/// [`qualify`], with `centres` giving each face's centre (worked out only when pieces tie): pieces
/// that nothing else tells apart are numbered along their spread, not in the kernel's order,
/// which can differ from one evaluation to the next (#3902). Without `centres`, in the kernel's
/// order and by the faces they came from, as builds before that numbered them.
fn qualify_at(named: Vec<Named>, adjacent: &[(usize, usize)], centres: Option<&dyn Fn() -> Vec<Vec3>>) -> Names {
    let by_place = centres.is_some();
    let worked_out = std::cell::OnceCell::new();
    // Tied pieces in the order of their spread; as they are when there are no centres.
    let in_order = |tied: Vec<usize>| -> Vec<usize> {
        let at: &Vec<Vec3> = worked_out.get_or_init(|| centres.map(|c| c()).unwrap_or_default());
        if at.len() != named.len() {
            return tied;
        }
        crate::naming::along_spread(tied.into_iter().filter_map(|i| Some((i, *at.get(i)?))).collect())
    };
    let n = named.len();
    let mut bounds: Vec<BTreeSet<String>> = vec![BTreeSet::new(); n];
    for &(a, b) in adjacent {
        let (Some(na), Some(nb)) = (named.get(a), named.get(b)) else { continue };
        let (sa, sb) = (crate::naming::strip_piece(&na.name), crate::naming::strip_piece(&nb.name));
        if a == b || sa == sb {
            continue;
        }
        if let Some(set) = bounds.get_mut(a) {
            set.insert(sb.to_string());
        }
        if let Some(set) = bounds.get_mut(b) {
            set.insert(sa.to_string());
        }
    }
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, x) in named.iter().enumerate() {
        groups.entry(x.name.as_str()).or_default().push(i);
    }
    let mut names: Vec<String> = named.iter().map(|x| x.name.clone()).collect();
    let mut verified: Vec<bool> = named.iter().map(|x| x.verified).collect();
    for (base, members) in groups.iter().filter(|(_, m)| m.len() > 1) {
        let empty = BTreeSet::new();
        let common: BTreeSet<&String> = members
            .iter()
            .map(|i| bounds.get(*i).unwrap_or(&empty))
            .fold(None::<BTreeSet<&String>>, |acc, set| {
                Some(match acc {
                    None => set.iter().collect(),
                    Some(acc) => acc.into_iter().filter(|x| set.contains(*x)).collect(),
                })
            })
            .unwrap_or_default();
        let own = |i: usize| -> Vec<&String> { bounds.get(i).unwrap_or(&empty).iter().filter(|x| !common.contains(x)).collect() };
        // The face's stem, the faces the parent's qualifier lists (the bounds that told the
        // parent apart from its siblings) and the number that told apart the pieces nothing
        // else told apart.
        let (stem, inherited, tie) = split_piece(base);
        let mut by_key: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for &i in members {
            let own = own(i);
            // Nothing around it of its own: the piece keeps the name it had.
            let name = if own.is_empty() {
                base.to_string()
            } else {
                // The bounds that told the parent apart and the bounds that tell this piece
                // apart: the name says which piece of the face it is without repeating the
                // qualifiers of the pieces before it (the name grew with every split
                // otherwise, #5860).
                let mut key: BTreeSet<&str> = inherited.iter().map(String::as_str).collect();
                key.extend(own.iter().map(|s| s.as_str()));
                let key: Vec<&str> = key.into_iter().collect();
                piece_name(&stem, &key, tie.as_deref())
            };
            by_key.entry(name).or_default().push(i);
        }
        for (key, tied) in by_key {
            if let [i] = tied.as_slice() {
                if let Some(slot) = names.get_mut(*i) {
                    *slot = key;
                }
                continue;
            }
            // The faces around them don't tell these apart: numbered by what they came from when
            // that differs (exact), else in the kernel's order (unverified).
            let mut tied = tied;
            let origins: Vec<Option<&Vec<FaceOrigin>>> = tied.iter().map(|i| named.get(*i).and_then(|x| x.origins.as_ref())).collect();
            let distinct = origins.iter().all(Option::is_some) && origins.iter().collect::<BTreeSet<_>>().len() == origins.len();
            if distinct && !by_place {
                tied.sort_by_key(|i| named.get(*i).and_then(|x| x.origins.clone()));
            } else if distinct {
                // By what they came from, but not by where it stood in its operand's face order:
                // an operand that came out of a boolean can list its faces in another order on
                // the next evaluation (#3902). Pieces of faces of one kind are numbered by place.
                let mut by_kind: BTreeMap<Vec<FaceOrigin>, Vec<usize>> = BTreeMap::new();
                for i in tied {
                    let kind = named.get(i).and_then(|x| x.origins.as_deref()).unwrap_or_default().iter().map(|o| without_index(*o)).collect();
                    by_kind.entry(kind).or_default().push(i);
                }
                tied = by_kind.into_values().flat_map(&in_order).collect();
            } else {
                tied = in_order(tied);
            }
            for (k, i) in tied.iter().enumerate() {
                if let Some(slot) = names.get_mut(*i) {
                    *slot = format!("{key}#{}", k + 1);
                }
                if let Some(v) = verified.get_mut(*i).filter(|_| !distinct) {
                    *v = false;
                }
            }
        }
    }
    // A piece's new name can meet a face that already had it: numbered too (along their spread),
    // never shared.
    let mut seen: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, name) in names.iter().enumerate() {
        seen.entry(name.clone()).or_default().push(i);
    }
    for (name, same) in seen.into_iter().filter(|(_, same)| same.len() > 1) {
        for (k, i) in in_order(same).into_iter().enumerate() {
            if let Some(slot) = names.get_mut(i) {
                *slot = format!("{name}#{}", k + 1);
            }
            if let Some(v) = verified.get_mut(i) {
                *v = false;
            }
        }
    }
    let info = named
        .into_iter()
        .zip(bounds)
        .zip(verified)
        .map(|((x, bounds), verified)| FaceInfo { aliases: x.aliases, synonyms: x.synonyms, bounds, verified })
        .collect();
    Names { faces: Arc::new(names), info }
}

/// The most bytes a face name may take: a command takes a face name of at most this many
/// bytes, so a longer name could not be used to find the face again.
const MAX_NAME_BYTES: usize = 512;

/// A piece's name: the face's stem, the faces around the piece that tell it apart, as many
/// of them as the name can hold (`MAX_NAME_BYTES`), and the number that told the pieces of
/// the same face apart.
fn piece_name(stem: &str, faces: &[&str], tie: Option<&str>) -> String {
    let mut kept: Vec<&str> = Vec::with_capacity(faces.len());
    // The stem, the `#[` and the `]` the qualifier is written in.
    let mut len = stem.len() + 3;
    for face in faces {
        let more = face.len() + if kept.is_empty() { 0 } else { 1 };
        if len + more > MAX_NAME_BYTES {
            break;
        }
        len += more;
        kept.push(face);
    }
    let mut name = if kept.is_empty() { stem.to_string() } else { format!("{stem}#[{}]", kept.join(",")) };
    if let Some(tie) = tie {
        name.push('#');
        name.push_str(tie);
    }
    name
}

/// A piece name's parts: the face's stem, the faces its qualifier lists and the number that
/// tells apart the pieces of the same face that nothing else tells apart
/// (`F2:top#[F3:side]#2`). A name that cannot be read is taken as it is: a hostile name never
/// panics.
fn split_piece(name: &str) -> (String, Vec<String>, Option<String>) {
    let (stem, around) = match crate::piece_names::piece_parts(name) {
        Some((stem, around)) => (stem.to_string(), around.iter().map(|face| face.to_string()).collect()),
        None => (name.to_string(), Vec::new()),
    };
    (stem, around, tie_number(name))
}

/// The number at the end of a piece name (`#2` of `F2:top#[F3:side]#2`), when it has one: it
/// tells apart the pieces of a face that nothing else tells apart.
fn tie_number(name: &str) -> Option<String> {
    let i = name.rfind('#')?;
    let digits = name.get(i + 1..)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(digits.to_string())
}

/// `origin` with the face or edge it names left out (its kind and operand kept).
fn without_index(origin: FaceOrigin) -> FaceOrigin {
    match origin {
        FaceOrigin::Face { operand, .. } => FaceOrigin::Face { operand, face: 0 },
        FaceOrigin::Edge { operand, .. } => FaceOrigin::Edge { operand, edge: 0 },
        FaceOrigin::Offset { operand, .. } => FaceOrigin::Offset { operand, face: 0 },
        // A profile's segments come in the sketch's order.
        FaceOrigin::Profile { .. } | FaceOrigin::Cap { .. } => origin,
    }
}

/// How a stored face name matches a current face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Match {
    /// Only its root stem is the same: some piece of the same face (the caller picks by point).
    Stem,
    /// A piece of the named face, or the named piece told apart by the faces around it.
    Piece,
    /// One of the faces merged into this one.
    Alias,
    /// The very name.
    Exact,
}

/// How the stored name `name` matches face `index` (`None`: not at all).
pub(crate) fn matches(names: &Names, index: usize, name: &str) -> Option<Match> {
    let current = names.faces.get(index)?;
    let info = names.info.get(index);
    if current == name {
        return Some(Match::Exact);
    }
    // Another name of this very face; of a piece of it, when the face was split since.
    if info.is_some_and(|i| i.synonyms.iter().any(|a| a == name)) {
        return Some(if crate::naming::strip_piece(current) == current { Match::Exact } else { Match::Piece });
    }
    if info.is_some_and(|i| i.aliases.iter().any(|a| a == name)) {
        return Some(Match::Alias);
    }
    // A pattern copy's faces carry their instance after the name (`…@F9.1`, after any
    // qualifier): the names compare as the original's do, within the same copy only.
    let ((name, at), (current, here)) = (instance_suffix(name), instance_suffix(current));
    if at != here {
        return None;
    }
    // The named face (or piece) was split further: this is one of its pieces.
    if descends(current, name) {
        return Some(Match::Piece);
    }
    // The named piece, told apart by the faces around it: they still bound this piece.
    if let Some((stem, around)) = crate::piece_names::piece_parts(name)
        && crate::naming::parent_piece(current) == Some(stem)
        && info.is_some_and(|i| around.iter().all(|f| i.bounds.contains(*f)))
    {
        return Some(Match::Piece);
    }
    let root = crate::naming::strip_piece(name);
    (crate::naming::strip_piece(current) == root).then_some(Match::Stem)
}

/// A name and the pattern instances after it (`F3:top#[F4:side]@F9.1` is `F3:top#[F4:side]` and
/// `@F9.1`; a copy of a copy carries one instance after another).
fn instance_suffix(n: &str) -> (&str, &str) {
    let mut core = n;
    while let Some(at) = core.rfind('@') {
        let instance = core.get(at + 1..).and_then(|t| t.strip_prefix('F')).and_then(|t| t.split_once('.'));
        let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
        if !instance.is_some_and(|(feature, k)| digits(feature) && digits(k)) {
            break;
        }
        core = core.get(..at).unwrap_or_default();
    }
    (core, n.get(core.len()..).unwrap_or_default())
}

#[cfg(test)]
#[path = "history_names_tests.rs"]
mod tests;
