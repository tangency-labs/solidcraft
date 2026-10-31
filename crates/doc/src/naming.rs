//! Persistent names of faces and edges.
//!
//! A fillet stores the edges it rounds, a sketch the face it sits on: by name, so that an
//! upstream edit that adds, removes, splits or reorders faces does not move them to another
//! face. Every face of an evaluated body is named after how it was made: a face that came from a
//! face of the feature's input keeps that face's name; a face new to the feature is named by its
//! role (`F<feature>:side:<sketch curve>`, `F<feature>:start`/`top`/`bottom`/`end`,
//! `F<feature>:blend:<edge>`, `F<feature>:inner:<face>`, …). Pieces of a split face are told
//! apart by the faces around them (`F2:top#[F2:side:l4,F4:side:l4]`). Edges are named by the
//! faces they separate (`<face A>|<face B>`, `~k` for several edges between the same two).
//!
//! Where the feature that made a body reports exact history (`crate::lineage`), the names follow
//! it (`crate::history_names`). Bodies without history (made by operations that do not report
//! it yet, imported meshes, legacy chains) are named from geometry: sample points of each face
//! are looked up on the input's faces. Those names are marked unverified, and references that
//! resolve through them say so. Names are worked out lazily, on first request.

use std::sync::{Arc, OnceLock};

use solidcraft_geom::{Aabb3, Mesh, Vec2, Vec3};

use crate::history_names::Names;
use crate::lineage::Lineage;
use crate::{Feature, FeatureKind, ModelBody, ModelState};

/// How a body came to be: the feature and the model just before it.
#[derive(Debug)]
pub struct NamingOrigin {
    pub feature: Feature,
    pub before: Arc<ModelState>,
    pub depth: usize,
    /// Instance transforms of a pattern or mirror (its copies are named after their sources).
    pub mats: Vec<crate::Mat>,
    /// The sheet metal bodies after a sheet feature (their folds place moved faces).
    pub sheets: Vec<crate::sheet::SheetBody>,
}

/// The lazily computed names of a body, shared by its clones.
#[derive(Clone, Debug, Default)]
pub struct NameCell {
    pub origin: Option<Arc<NamingOrigin>>,
    /// What the body's faces came from, when the feature that made it reports it.
    lineage: Option<Arc<Lineage>>,
    names: Arc<OnceLock<Arc<Names>>>,
    /// The names designs saved before format 14 gave its faces (see [`LEGACY_MARK`]).
    legacy: Arc<OnceLock<Arc<Vec<String>>>>,
    /// The body this one is a moved copy of (its legacy names are that body's).
    copy_of: Option<Arc<ModelBody>>,
    /// How deep the chain of features that named it is, for names taken from a saved model
    /// (which has no origin): see [`NameCell::depth`].
    saved_depth: Option<usize>,
}

impl NameCell {
    pub fn new(origin: Option<Arc<NamingOrigin>>) -> Self {
        Self::traced(origin, None)
    }
    /// Names of a body made by `origin`'s feature, following `lineage` where it is known.
    pub(crate) fn traced(origin: Option<Arc<NamingOrigin>>, lineage: Option<Arc<Lineage>>) -> Self {
        NameCell { origin, lineage, names: Arc::new(OnceLock::new()), legacy: Arc::new(OnceLock::new()), copy_of: None, saved_depth: None }
    }
    /// Names already known, taken as they are.
    pub fn known(names: Arc<Vec<String>>) -> Self {
        let legacy = OnceLock::new();
        let _ = legacy.set(names.clone());
        let mut cell = Self::shared(Arc::new(Names::given(names)));
        cell.legacy = Arc::new(legacy);
        cell
    }
    /// The names of `body` (a moved copy of a body keeps its faces' names, and how sure they are).
    ///
    /// Worked out on first request, from the source: moving a body (a transfer into a
    /// component, a placed instance) does not name every face of every moved body up front.
    pub fn same(body: &ModelBody) -> Self {
        let mut cell = Self::traced(None, None);
        cell.copy_of = Some(Arc::new(body.clone()));
        cell
    }
    /// Names a saved model kept (worked out when it was saved), at `depth` in the chain of
    /// features that named them.
    pub(crate) fn saved(names: Arc<Names>, depth: Option<usize>) -> Self {
        let mut cell = Self::shared(names);
        cell.saved_depth = depth;
        cell
    }
    /// How deep the chain of features that named the body is (`None`: not made by a feature).
    pub(crate) fn depth(&self) -> Option<usize> {
        self.origin.as_ref().map(|o| o.depth).or(self.saved_depth)
    }
    /// Identity of the names: clones of a cell share them.
    pub(crate) fn id(&self) -> usize {
        Arc::as_ptr(&self.names) as usize
    }
    /// The same origin, lineage and depth with nothing worked out yet: for a body given a new
    /// name, whose faces' names may be made from it.
    pub(crate) fn renamed(&self) -> Self {
        let mut cell = Self::traced(self.origin.clone(), self.lineage.clone());
        cell.copy_of = self.copy_of.clone();
        cell.saved_depth = self.saved_depth;
        cell
    }
    fn shared(names: Arc<Names>) -> Self {
        let cell = OnceLock::new();
        let _ = cell.set(names);
        NameCell { origin: None, lineage: None, names: Arc::new(cell), legacy: Arc::new(OnceLock::new()), copy_of: None, saved_depth: None }
    }
}

// Origins retain older model states, whose bodies retain older origins. Default
// Arc destruction therefore recurses through the entire timeline when the last
// model/tab is closed. The naming-computation depth limit does not bound this
// destruction path. Reclaim only uniquely owned ancestors, using a heap worklist;
// shared snapshots and their naming data are never edited or prematurely freed.
enum NamingAncestor {
    State(Arc<ModelState>),
    Copy(Arc<ModelBody>),
}
impl NameCell {
    fn detach_ancestors(&mut self, pending: &mut Vec<NamingAncestor>) {
        if let Some(origin) = self.origin.take().and_then(Arc::into_inner) {
            pending.push(NamingAncestor::State(origin.before));
        }
        if let Some(source) = self.copy_of.take() {
            pending.push(NamingAncestor::Copy(source));
        }
    }
}
impl Drop for NameCell {
    fn drop(&mut self) {
        if self.origin.is_none() && self.copy_of.is_none() {
            return;
        }
        let mut pending = Vec::new();
        self.detach_ancestors(&mut pending);
        while let Some(ancestor) = pending.pop() {
            match ancestor {
                NamingAncestor::State(state) => {
                    if let Some(mut state) = Arc::into_inner(state) {
                        for body in &mut state.bodies {
                            body.names.detach_ancestors(&mut pending);
                        }
                    }
                }
                NamingAncestor::Copy(body) => {
                    if let Some(mut body) = Arc::into_inner(body) {
                        body.names.detach_ancestors(&mut pending);
                    }
                }
            }
        }
        // Arc::into_inner, unlike try_unwrap(...).ok(), cannot accidentally
        // recursively drop the inner value after a racing final-owner release.
        // Ordinary naming caches/lineage remain governed by their original Arcs.
    }
}

pub use crate::name_lookup::{EdgeHit, FaceHit, LEGACY_MARK, face_has_name, faces_with_stem, legacy_name, locate_edge, locate_face, locate_one};
pub use crate::piece_names::parent_piece;

/// Deeper chains than this are named by position only (bounded work and stack).
const MAX_DEPTH: usize = 400;

/// Up to this many sample points per face.
const SAMPLES: usize = 9;

/// Up to this many sample points per face where a sheet's panel kept in place names the face it is
/// part of (see [`geometric_from`]).
const PANEL_SAMPLES: usize = 256;

/// Unit normals of each face's triangles.
fn normals(m: &Mesh, nf: usize) -> Vec<Vec<Vec3>> {
    let mut per: Vec<Vec<Vec3>> = vec![Vec::new(); nf];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        if let (Some(v), Some([a, b, c])) = (per.get_mut(*f as usize), m.tri(t))
            && let Some(n) = (b - a).cross(c - a).normalized()
        {
            v.push(n);
        }
    }
    per
}

/// Triangle centroids spread over each face: face → points.
fn samples(m: &Mesh, nf: usize) -> Vec<Vec<Vec3>> {
    samples_up_to(m, nf, SAMPLES)
}

/// [`samples`] with up to `most` points per face.
fn samples_up_to(m: &Mesh, nf: usize, most: usize) -> Vec<Vec<Vec3>> {
    let mut per: Vec<Vec<Vec3>> = vec![Vec::new(); nf];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        if let (Some(v), Some([a, b, c])) = (per.get_mut(*f as usize), m.tri(t)) {
            v.push((a + b + c) * (1.0 / 3.0));
        }
    }
    per.into_iter()
        .map(|v| {
            if v.len() <= most {
                return v;
            }
            let step = v.len() as f64 / most as f64;
            (0..most).filter_map(|i| v.get((i as f64 * step) as usize).copied()).collect()
        })
        .collect()
}

/// The point of triangle `abc` nearest `p`. A zero-length edge (a cone's tip repeats its apex
/// vertex) has no interior to project onto, so its branch is skipped rather than dividing 0 by 0.
pub(crate) fn closest(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 && d1 - d3 > 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 && d2 - d6 > 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 && (d4 - d3) + (d5 - d6) > 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let den = va + vb + vc;
    if den.abs() < 1e-300 {
        return a;
    }
    a + ab * (vb / den) + ac * (vc / den)
}

/// A body's faces as triangle lists with bounding boxes, for "does this point lie on face f".
struct FaceIndex {
    faces: Vec<(Aabb3, Vec<[Vec3; 3]>)>,
}

impl FaceIndex {
    fn new(m: &Mesh, nf: usize) -> FaceIndex {
        let mut faces: Vec<(Aabb3, Vec<[Vec3; 3]>)> = vec![(Aabb3::EMPTY, Vec::new()); nf];
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            if let (Some(slot), Some(tri)) = (faces.get_mut(*f as usize), m.tri(t)) {
                slot.0.extend(tri);
                slot.1.push(tri);
            }
        }
        FaceIndex { faces }
    }

    /// The face a point lies on (within `tol`).
    fn face_at(&self, p: Vec3, tol: f64) -> Option<usize> {
        let mut best: Option<(f64, usize)> = None;
        for (i, (bounds, tris)) in self.faces.iter().enumerate() {
            if !bounds.inflated(tol).contains(p) {
                continue;
            }
            let d = tris.iter().map(|[a, b, c]| closest(p, *a, *b, *c).dist(p)).fold(f64::MAX, f64::min);
            if d <= tol && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }
}

fn face_count(b: &ModelBody) -> usize {
    b.mesh().tri_face.iter().map(|f| *f as usize + 1).max().unwrap_or(0)
}

/// Names of a body's faces (by B-rep face index).
pub fn face_names(b: &ModelBody) -> Arc<Vec<String>> {
    names(b).faces.clone()
}

/// Whether face `index`'s name follows exact history (`false`: worked out from geometry, which an
/// upstream edit can fool).
pub fn face_name_exact(b: &ModelBody, index: usize) -> bool {
    names(b).verified(index)
}

/// Prepare only names not yet known before a lookup reads several bodies. A ready cell is
/// shared by snapshots and poses: dispatching threads just to read it costs more than a lookup.
pub(crate) fn warm_names<'a>(bodies: impl Iterator<Item = &'a ModelBody>) {
    let bodies: Vec<_> = bodies.filter(|b| b.names.names.get().is_none()).collect();
    #[cfg(test)]
    tests::record_warm_jobs(bodies.len());
    match bodies.as_slice() {
        [] => {}
        [body] => {
            let _ = names(body);
        }
        _ => solidcraft_kernel::for_each_on_threads(&bodies, 16, |b| {
            let _ = names(b);
        }),
    }
}

/// Bodies whose names are worked out one from the other, oldest first, at most this many apart
/// (see [`warm_chain`]).
const WARM_STEP: usize = 16;

/// Up to this many earlier versions of a body are walked to name it from the oldest up.
const MAX_CHAIN: usize = 100_000;

/// The body `b`'s names are worked out from first: the body it is a moved copy of, else the
/// body of its name before the feature that made it (`None`: none, or past [`MAX_DEPTH`]).
fn predecessor(b: &ModelBody) -> Option<&ModelBody> {
    if let Some(source) = &b.names.copy_of {
        return Some(source);
    }
    let o = b.names.origin.as_ref().filter(|o| o.depth < MAX_DEPTH)?;
    o.before.body(&b.name)
}

/// Work out the names of the earlier versions of `b` that are not known yet, oldest first and
/// every [`WARM_STEP`]th, so that naming `b` never recurses through more than that many of them.
/// A body's names are found from the names of the body it was made from, which are found the
/// same way: a body moved or changed by a few hundred features in a row whose names nothing
/// asked for overflowed the window thread's stack the first time something did (a fillet after
/// them evaluated again when an earlier feature is reordered or moved: #7587, #7926). `ready` says whether a
/// body's names are known, `name` works them out. The names are the same either way.
fn warm_chain(b: &ModelBody, ready: fn(&ModelBody) -> bool, name: fn(&ModelBody)) {
    let mut chain: Vec<&ModelBody> = Vec::new();
    let mut at = b;
    while chain.len() < MAX_CHAIN
        && let Some(p) = predecessor(at)
        && !ready(p)
    {
        chain.push(p);
        at = p;
    }
    if chain.len() <= WARM_STEP {
        return;
    }
    for p in chain.iter().rev().step_by(WARM_STEP) {
        name(p);
    }
}

/// A body's names, with what finding them again needs: from the history of the feature that
/// made it where it reports one, else from geometry (unverified).
pub(crate) fn names(b: &ModelBody) -> Arc<Names> {
    if let Some(known) = b.names.names.get() {
        return known.clone();
    }
    warm_chain(
        b,
        |x| x.names.names.get().is_some(),
        |x| {
            let _ = names(x);
        },
    );
    // A moved copy has its source's names (the source's cell keeps them for both).
    if let Some(source) = &b.names.copy_of {
        return b.names.names.get_or_init(|| names(source)).clone();
    }
    b.names
        .names
        .get_or_init(|| {
            // Exact history needs no mesh; geometry does (its face count is the mesh's).
            let names = match b.names.origin.as_ref() {
                Some(o) if o.depth < MAX_DEPTH => b.names.lineage.as_ref().and_then(|l| crate::history_names::derive(b, o, l)).unwrap_or_else(|| {
                    let centres = || face_centres(b);
                    Names::unverified(geometric_names(b, o, face_count(b)), &mesh_adjacency(b), crate::history_names::by_place(&o.feature, &centres))
                }),
                _ => Names::unverified((0..face_count(b)).map(|k| format!("{}:face:{k}", b.name)).collect(), &[], None),
            };
            Arc::new(names)
        })
        .clone()
}

/// The names builds before format 14 gave a body's faces: from geometry, over the inputs' names
/// as those builds gave them, pieces of a split face numbered along the longest axis of their
/// spread (`#1`, `#2`, …).
pub(crate) fn legacy_face_names(b: &ModelBody) -> Arc<Vec<String>> {
    if let Some(known) = b.names.legacy.get() {
        return known.clone();
    }
    warm_chain(
        b,
        |x| x.names.legacy.get().is_some(),
        |x| {
            let _ = legacy_face_names(x);
        },
    );
    if let Some(source) = &b.names.copy_of {
        return legacy_face_names(source);
    }
    b.names
        .legacy
        .get_or_init(|| {
            let nf = face_count(b);
            let names = match b.names.origin.as_ref() {
                Some(o) if o.depth < MAX_DEPTH => {
                    let mut names = geometric_from(b, o, nf, legacy_face_names);
                    number_pieces(&mut names, &samples(&b.mesh(), nf), '#');
                    names
                }
                _ => (0..nf).map(|k| format!("{}:face:{k}", b.name)).collect(),
            };
            Arc::new(names)
        })
        .clone()
}

/// The names builds before format 14 gave a body's edges.
pub(crate) fn legacy_edge_names(b: &ModelBody) -> Vec<String> {
    edge_names_from(b, &legacy_face_names(b))
}

/// Pairs of faces of a body that share an edge (from its mesh's edges).
pub(crate) fn mesh_adjacency(b: &ModelBody) -> Vec<(usize, usize)> {
    crate::history_names::face_pairs(b.mesh().edge_faces.iter().map(|fs| fs.iter().map(|f| *f as usize)))
}

/// Work out a body's face names from the feature that made it and the model before it, by
/// geometry: where each face's sample points lie on the input's faces. Faces sharing a name are
/// pieces of one, told apart afterwards.
pub(crate) fn geometric_names(b: &ModelBody, o: &NamingOrigin, nf: usize) -> Vec<String> {
    geometric_from(b, o, nf, face_names)
}

/// [`geometric_names`], the input bodies' faces named by `names_of`.
fn geometric_from(b: &ModelBody, o: &NamingOrigin, nf: usize, names_of: fn(&ModelBody) -> Arc<Vec<String>>) -> Vec<String> {
    // A rigid Move changes coordinates, not topology or face ownership. Geometric
    // overlap with the unmoved input cannot recover the names at the new position.
    if matches!(o.feature.kind, FeatureKind::Move { .. })
        && let Some(input) = o.before.body(&b.name)
    {
        return names_of(input).as_ref().clone();
    }
    let m = b.mesh();
    let size = b.body.size().max(1e-6);
    let tol = (size * 1e-4).max(1e-6);
    let pts = samples(&m, nf);
    let bb = m.bounds();
    // Inputs that overlap this body.
    let overlaps = |x: &ModelBody| {
        let xb = x.mesh().bounds();
        !(xb.max.x < bb.min.x - tol
            || xb.min.x > bb.max.x + tol
            || xb.max.y < bb.min.y - tol
            || xb.min.y > bb.max.y + tol
            || xb.max.z < bb.min.z - tol
            || xb.min.z > bb.max.z + tol)
    };
    // Independently created solids own their faces even when another body
    // occupies the same coordinates. Boolean operations still inherit inputs.
    let independent_solid = matches!(
        o.feature.kind,
        FeatureKind::Box { operation: crate::Operation::NewBody, .. }
            | FeatureKind::Cylinder { operation: crate::Operation::NewBody, .. }
            | FeatureKind::Sphere { operation: crate::Operation::NewBody, .. }
            | FeatureKind::Torus { operation: crate::Operation::NewBody, .. }
            | FeatureKind::Cone { operation: crate::Operation::NewBody, .. }
            | FeatureKind::Extrude { operation: crate::Operation::NewBody, .. }
            | FeatureKind::Revolve { operation: crate::Operation::NewBody, .. }
    );
    let inputs: Vec<(&ModelBody, FaceIndex, Arc<Vec<String>>)> = o
        .before
        .bodies
        .iter()
        .filter(|x| !independent_solid && !x.body.is_mesh() && overlaps(x))
        .take(64)
        .map(|x| {
            let n = face_count(x);
            (x, FaceIndex::new(&x.mesh(), n), names_of(x))
        })
        .collect();
    let fid = o.feature.id;
    let ns = normals(&m, nf);
    let mut names: Vec<Option<String>> = pts.iter().map(|ps| vote(ps, &inputs, tol, None)).collect();
    // A pattern or mirror copy: its faces lie on its source's faces moved by one instance.
    let found = names.iter().filter(|n| n.is_some()).count();
    if !o.mats.is_empty() && found * 2 < nf.max(1) {
        let all: Vec<(&ModelBody, FaceIndex, Arc<Vec<String>>)> = o
            .before
            .bodies
            .iter()
            .filter(|x| !x.body.is_mesh())
            .take(64)
            .map(|x| (x, FaceIndex::new(&x.mesh(), face_count(x)), names_of(x)))
            .collect();
        let mut best: Option<(usize, usize, Vec<Option<String>>)> = None;
        for (k, mat) in o.mats.iter().enumerate() {
            let Some(inv) = crate::mat_inverse(mat) else { continue };
            let got: Vec<Option<String>> = pts.iter().map(|ps| vote(ps, &all, tol, Some(&inv))).collect();
            let n = got.iter().filter(|x| x.is_some()).count();
            if n > found && best.as_ref().is_none_or(|(bn, _, _)| n > *bn) {
                best = Some((n, k, got));
            }
        }
        if let Some((_, k, got)) = best {
            names = got.into_iter().map(|n| n.map(|n| format!("{n}@F{fid}.{}", k + 1))).collect();
        }
    }
    // A sheet feature that moved panels (a fold, an unfold): faces it carried along lie on the
    // faces they were, taken back through the panel they are on.
    if let (Some(after), Some(before), Some(input)) = (
        o.sheets.iter().find(|s| s.body == b.name),
        o.before.sheets.iter().find(|s| s.body == b.name),
        inputs.iter().find(|(x, _, _)| x.name == b.name),
    ) {
        let input = std::slice::from_ref(input);
        let mut dense: Option<Vec<Vec<Vec3>>> = None;
        for (fi, (name, ps)) in names.iter_mut().zip(&pts).enumerate() {
            if name.is_some() {
                continue;
            }
            // The panel the feature kept in place (the base an unfold lays the flanges out beside)
            // lies on its own face before, and that face names the whole face it is part of now:
            // a flat face spans that panel, the panels laid out beside it and the bend strips
            // between them, whatever share of it they take. The few sample points above may all
            // miss it (a bend strip is many small triangles), so the face is looked at closer.
            let dense = dense.get_or_insert_with(|| samples_up_to(&m, nf, PANEL_SAMPLES));
            if let Some(kept) = dense.get(fi).and_then(|ps| plurality(ps, input, tol)) {
                *name = Some(kept);
                continue;
            }
            // A point on a bend lies on no panel and has no place before; the face's other points
            // do. At least half of them must, so a face that is mostly bend is not named from a
            // few points.
            let back: Vec<Vec3> = ps.iter().filter_map(|p| after.point_before(before, *p)).collect();
            if !back.is_empty() && back.len() * 2 >= ps.len() {
                *name = vote(&back, input, tol, None);
            }
        }
    }
    let empty = Vec::new();
    let curves = std::cell::OnceCell::new();
    let names: Vec<String> = names
        .into_iter()
        .enumerate()
        .map(|(fi, n)| {
            n.unwrap_or_else(|| {
                let (ps, nn) = (pts.get(fi).map(Vec::as_slice).unwrap_or(&[]), ns.get(fi).unwrap_or(&empty));
                // A shell's inner face: the face its wall runs back to.
                if matches!(o.feature.kind, FeatureKind::Shell { .. })
                    && let Some(n) = behind(ps, nn, &inputs)
                {
                    return format!("F{fid}:inner:{}", strip_piece(&n));
                }
                role_name(&o.feature, &o.before, ps, nn, fi, fid, &curves)
            })
        })
        .collect();
    names
}

/// The input face a face looks back at through the material (along minus its normal): the
/// face a shell wall's inner side offsets.
fn behind(ps: &[Vec3], ns: &[Vec3], inputs: &[(&ModelBody, FaceIndex, Arc<Vec<String>>)]) -> Option<String> {
    let n = ns.first().copied()?;
    let d = n * -1.0;
    let mut votes: Vec<(String, usize)> = Vec::new();
    for p in ps {
        let mut best: Option<(f64, String)> = None;
        for (_, idx, nm) in inputs {
            for (fi, (_, tris)) in idx.faces.iter().enumerate() {
                for [a, b, c] in tris {
                    if let Some(t) = ray_hit(*p, d, *a, *b, *c)
                        && t > 1e-9
                        && best.as_ref().is_none_or(|(bt, _)| t < *bt)
                        && let Some(name) = nm.get(fi)
                    {
                        best = Some((t, name.clone()));
                    }
                }
            }
        }
        if let Some((_, n)) = best {
            match votes.iter_mut().find(|(v, _)| *v == n) {
                Some(v) => v.1 += 1,
                None => votes.push((n, 1)),
            }
        }
    }
    votes.sort_by_key(|v| std::cmp::Reverse(v.1));
    votes.first().filter(|(_, k)| *k * 2 >= ps.len().max(1)).map(|(n, _)| n.clone())
}

pub(crate) fn ray_hit(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f64> {
    let (e1, e2) = (b - a, c - a);
    let h = d.cross(e2);
    let det = e1.dot(h);
    if det.abs() < 1e-14 {
        return None;
    }
    let f = 1.0 / det;
    let s = o - a;
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = f * d.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some(f * e2.dot(q))
}

/// The input face most of a face's sample points lie on (mapped by `map` first), if it holds
/// at least half of them.
fn vote(ps: &[Vec3], inputs: &[(&ModelBody, FaceIndex, Arc<Vec<String>>)], tol: f64, map: Option<&crate::Mat>) -> Option<String> {
    let mut votes: Vec<(String, usize)> = Vec::new();
    for p in ps {
        let p = match map {
            Some(m) => crate::apply_point(m, *p),
            None => *p,
        };
        for (_, idx, nm) in inputs {
            if let Some(f) = idx.face_at(p, tol)
                && let Some(n) = nm.get(f)
            {
                // A later operation may merge several pieces of the same input face.
                // Count their shared ancestry before testing the majority; a piece
                // number identifies topology subdivision, not a different source.
                let n = strip_piece(n);
                match votes.iter_mut().find(|(v, _)| v == n) {
                    Some(v) => v.1 += 1,
                    None => votes.push((n.to_string(), 1)),
                }
                break;
            }
        }
    }
    votes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    votes.first().filter(|(_, n)| *n * 2 >= ps.len().max(1) && votes.get(1).is_none_or(|(_, other)| n > other)).map(|(n, _)| n.clone())
}

/// The input face the most of `ps` lie on (pieces counted as their face), if at least two and a
/// sixteenth of them lie on one: the face a sheet's unmoved panel was part of.
fn plurality(ps: &[Vec3], inputs: &[(&ModelBody, FaceIndex, Arc<Vec<String>>)], tol: f64) -> Option<String> {
    let mut votes: Vec<(String, usize)> = Vec::new();
    for p in ps {
        for (_, idx, nm) in inputs {
            if let Some(f) = idx.face_at(*p, tol)
                && let Some(n) = nm.get(f)
            {
                let n = strip_piece(n);
                match votes.iter_mut().find(|(v, _)| v == n) {
                    Some(v) => v.1 += 1,
                    None => votes.push((n.to_string(), 1)),
                }
                break;
            }
        }
    }
    votes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    votes.first().filter(|(_, n)| *n >= 2 && *n * 16 >= ps.len()).map(|(n, _)| n.clone())
}

/// A face name without its piece qualifiers: the face it is a piece of (`F2:top` for
/// `F2:top#[F4:side:l1]`, for pieces of pieces, and for the numbered pieces of older designs).
pub fn strip_piece(n: &str) -> &str {
    let mut stem = n;
    while let Some(parent) = parent_piece(stem) {
        stem = parent;
    }
    stem
}

/// An edge name without piece numbers: its own (`~k`) and its faces' (`#[…]`, and the numbered
/// pieces of older designs), so pieces of a split edge, or of an edge between split faces, match.
pub fn strip_edge_piece(n: &str) -> String {
    crate::piece_names::strip_edge_piece(n)
}

/// Edges sharing a name: number them along the longest axis of their spread.
fn number_pieces(names: &mut [String], pts: &[Vec<Vec3>], mark: char) {
    // The pieces of each name, gathered in one pass (a faceted body has thousands of names).
    let mut groups: std::collections::BTreeMap<String, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, n) in names.iter().enumerate() {
        groups.entry(n.clone()).or_default().push(i);
    }
    for (n, same) in groups {
        if same.len() < 2 {
            continue;
        }
        let centre = |j: usize| {
            let ps = pts.get(j).cloned().unwrap_or_default();
            ps.iter().fold(Vec3::ZERO, |a, p| a + *p) * (1.0 / ps.len().max(1) as f64)
        };
        let order = along_spread(same.iter().map(|j| (*j, centre(*j))).collect());
        for (k, j) in order.iter().enumerate() {
            if let Some(slot) = names.get_mut(*j) {
                *slot = format!("{n}{mark}{}", k + 1);
            }
        }
    }
}

/// The items of `cs` (index, centre) ordered along the longest axis of their centres' spread,
/// then the next: how the pieces of one face or edge are numbered, the same on every run
/// whatever order the kernel gives them in.
pub(crate) fn along_spread(cs: Vec<(usize, Vec3)>) -> Vec<usize> {
    let ext = cs.iter().map(|(_, c)| *c).collect::<Aabb3>().size();
    let key = |c: Vec3| {
        if ext.x >= ext.y && ext.x >= ext.z {
            (c.x, c.y, c.z)
        } else if ext.y >= ext.z {
            (c.y, c.z, c.x)
        } else {
            (c.z, c.x, c.y)
        }
    };
    let mut order = cs;
    // Pieces lying almost level along the sort axis differ only in the last bits of their centre:
    // the centre is the mean of a tessellated boundary, and that depends on where the loop's
    // polyline starts, which can differ between processes. Comparing the keys with a tolerance
    // makes those pieces equal, so the stable sort keeps the order they arrived in and a name
    // keeps its number across runs (solidcraft#569).
    let tol = ext.x.max(ext.y).max(ext.z).max(1.0) * 1e-9;
    let bucket = |c: Vec3| {
        let k = key(c);
        // Rounding onto the grid, rather than comparing with a tolerance, is what keeps this a
        // total order: `a` and `b` in one bucket and `b` and `c` in the next must not leave `a`
        // and `c` in different ones, or the sort has no order to work with. Since Rust 1.81
        // `sort_by` may panic on a comparator that is not one. Two pieces can still flip when
        // they straddle a bucket edge; that is the price of a key that ignores their low bits.
        ((k.0 / tol).round() as i64, (k.1 / tol).round() as i64, (k.2 / tol).round() as i64)
    };
    order.sort_by_key(|a| bucket(a.1));
    order.into_iter().map(|(j, _)| j).collect()
}

/// Each face's centre: the mean of the sampled centres of its triangles.
pub(crate) fn face_centres(b: &ModelBody) -> Vec<Vec3> {
    samples(&b.mesh(), face_count(b)).into_iter().map(|ps| ps.iter().fold(Vec3::ZERO, |a, p| a + *p) * (1.0 / ps.len().max(1) as f64)).collect()
}

/// The name of a face new to `f`.
fn role_name(f: &Feature, before: &ModelState, ps: &[Vec3], ns: &[Vec3], fi: usize, fid: u64, curves: &std::cell::OnceCell<Curves>) -> String {
    let c = ps.iter().fold(Vec3::ZERO, |a, p| a + *p) * (1.0 / ps.len().max(1) as f64);
    match &f.kind {
        FeatureKind::Extrude { sketch, .. } => {
            let Some(ss) = before.sketch(sketch.0) else { return format!("F{fid}:face:{fi}") };
            let n = ss.plane.normal();
            // A face across the extrusion is a cap; others sweep a sketch curve.
            let flat = !ns.is_empty() && ns.iter().all(|m| m.cross(n).len() < 1e-6);
            if flat {
                // On the sketch plane, above it or below it.
                return format!("F{fid}:{}", crate::lineage::extrude_cap_role((c - ss.plane.origin).dot(n)));
            }
            let q = ss.plane.to_local(c);
            curves
                .get_or_init(|| Curves::of(ss))
                .nearest(Vec2::new(q.x, q.y))
                .map(|(_, id)| id)
                .map(|id| format!("F{fid}:side:{id}"))
                .unwrap_or_else(|| format!("F{fid}:face:{fi}"))
        }
        FeatureKind::Revolve { sketch, axis, .. } => {
            let Some(ss) = before.sketch(sketch.0) else { return format!("F{fid}:face:{fi}") };
            let Ok((o2, d2)) = crate::eval::revolve_axis(ss, axis, before, &|name| crate::eval::construction_axis_in(None, before, name, 0)) else {
                return format!("F{fid}:face:{fi}");
            };
            let (o, d) = (ss.plane.to_world(o2), (ss.plane.to_world(o2 + d2) - ss.plane.to_world(o2)).normalized().unwrap_or(Vec3::Z));
            // A flat face holding the axis is an angle cap: on the sketch plane, or the far one.
            let flat = !ns.is_empty() && ns.windows(2).all(|w| w[0].cross(w[1]).len() < 1e-6) && ns.iter().all(|m| m.dot(d).abs() < 1e-6);
            if flat {
                let on_plane = ps.iter().all(|p| (*p - ss.plane.origin).dot(ss.plane.normal()).abs() < 1e-6 * (1.0 + p.len()));
                return format!("F{fid}:{}", if on_plane { "start" } else { "end" });
            }
            // A swept face: turn its centre back into the sketch plane and find the curve.
            let q = c - o;
            let t = q.dot(d);
            let r = (q - d * t).len();
            let u3 = ss.plane.normal().cross(d).normalized().unwrap_or(Vec3::X);
            let best = [u3, u3 * -1.0]
                .iter()
                .filter_map(|u| {
                    let w = ss.plane.to_local(o + d * t + *u * r);
                    let q2 = Vec2::new(w.x, w.y);
                    curves.get_or_init(|| Curves::of(ss)).nearest(q2)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            best.map(|(_, id)| format!("F{fid}:side:{id}")).unwrap_or_else(|| format!("F{fid}:face:{fi}"))
        }
        FeatureKind::Sweep { .. } | FeatureKind::Loft { .. } => {
            let sk = match &f.kind {
                FeatureKind::Sweep { sketch, .. } => Some(*sketch),
                FeatureKind::Loft { sections, .. } => sections.first().map(|x| x.sketch),
                _ => None,
            };
            let Some(ss) = sk.and_then(|k| before.sketch(k.0)) else { return format!("F{fid}:face:{fi}") };
            let n = ss.plane.normal();
            let h = |p: &Vec3| (*p - ss.plane.origin).dot(n);
            let flat = !ns.is_empty() && ns.windows(2).all(|w| w[0].cross(w[1]).len() < 1e-6);
            if flat && ps.iter().all(|p| h(p).abs() < 1e-6 * (1.0 + p.len())) {
                return format!("F{fid}:start");
            }
            if flat && ns.iter().all(|m| m.cross(n).len() < 1e-6) {
                return format!("F{fid}:end");
            }
            // A side: the profile curve under its sample nearest the start.
            let Some(p0) = ps.iter().min_by(|a, b| h(a).abs().total_cmp(&h(b).abs())) else { return format!("F{fid}:face:{fi}") };
            let q = ss.plane.to_local(*p0);
            curves
                .get_or_init(|| Curves::of(ss))
                .nearest(Vec2::new(q.x, q.y))
                .map(|(_, id)| id)
                .map(|id| format!("F{fid}:side:{id}"))
                .unwrap_or_else(|| format!("F{fid}:face:{fi}"))
        }
        FeatureKind::Box { .. } => {
            let n = ns.first().copied().unwrap_or(Vec3::Z);
            let axis = |v: f64, p: &str, m: &str| {
                if v > 0.5 {
                    Some(p.to_string())
                } else if v < -0.5 {
                    Some(m.to_string())
                } else {
                    None
                }
            };
            let role =
                axis(n.x, "+x", "-x").or_else(|| axis(n.y, "+y", "-y")).or_else(|| axis(n.z, "+z", "-z")).unwrap_or_else(|| format!("face:{fi}"));
            format!("F{fid}:{role}")
        }
        FeatureKind::Cylinder { axis, .. } => {
            let flat = !ns.is_empty() && ns.iter().all(|m| m.cross(axis.0).len() < 1e-6);
            let role = if !flat {
                "side"
            } else if ns.first().is_some_and(|m| m.dot(axis.0) > 0.0) {
                "top"
            } else {
                "bottom"
            };
            format!("F{fid}:{role}")
        }
        FeatureKind::Fillet { edges, .. } | FeatureKind::Chamfer { edges, .. } => {
            let k = edges.iter().enumerate().min_by(|a, b| a.1.dist(c).total_cmp(&b.1.dist(c))).map(|(k, _)| k).unwrap_or(0);
            match f.edge_names.get(k) {
                Some(e) => format!("F{fid}:blend:{e}"),
                None => format!("F{fid}:blend:{k}"),
            }
        }
        _ => format!("F{fid}:face:{fi}"),
    }
}

/// A sketch's (non-construction) curves as the polylines a face's name is looked up on, each with
/// its box: a face swept from one of many curves (a sheet of holes) is named without measuring
/// its distance to every curve.
struct Curves {
    segs: Vec<(String, Vec<Vec2>, (Vec2, Vec2))>,
}

impl Curves {
    fn of(ss: &crate::SolvedSketch) -> Self {
        let mut segs = Vec::new();
        for (ci, c) in ss.sketch.curves.iter().enumerate() {
            if c.construction {
                continue;
            }
            for s in ss.sketch.segs(ci) {
                let pts = s.polyline(1e-3);
                let Some(&first) = pts.first() else { continue };
                let b =
                    pts.iter().fold((first, first), |(lo, hi), p| (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y))));
                segs.push((c.id.clone(), pts, b));
            }
        }
        Self { segs }
    }

    /// The curve (id) nearest `q`, and how near: the first of the nearest in the sketch's order.
    /// A curve whose box is farther than the nearest so far (by more than rounding) can't be nearer.
    fn nearest(&self, q: Vec2) -> Option<(f64, String)> {
        let mut best: Option<(f64, &str)> = None;
        for (id, pts, b) in &self.segs {
            let (lo, hi) = b;
            let out = Vec2::new((lo.x - q.x).max(q.x - hi.x).max(0.0), (lo.y - q.y).max(q.y - hi.y).max(0.0));
            if best.is_some_and(|(bd, _)| out.len() > bd + 1e-9 * (1.0 + q.len())) {
                continue;
            }
            let d = polyline_dist(pts, q);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, id));
            }
        }
        best.map(|(d, id)| (d, id.to_string()))
    }
}

fn polyline_dist(pts: &[Vec2], q: Vec2) -> f64 {
    pts.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let d = b - a;
            let t = if d.len2() > 0.0 { ((q - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
            (a + d * t).dist(q)
        })
        .fold(f64::MAX, f64::min)
}

/// Names of a body's edges (by mesh edge index): `<face A>|<face B>`, numbered when repeated.
pub fn edge_names(b: &ModelBody) -> Vec<String> {
    edge_names_from(b, &face_names(b))
}

/// Names of a body's edges from its faces' names `faces`.
fn edge_names_from(b: &ModelBody, faces: &[String]) -> Vec<String> {
    let m = b.mesh();
    let mut names: Vec<String> = m
        .edge_faces
        .iter()
        .map(|fs| {
            let mut n: Vec<&str> = fs.iter().filter_map(|f| faces.get(*f as usize).map(String::as_str)).collect();
            n.sort();
            n.join("|")
        })
        .collect();
    let pts: Vec<Vec<Vec3>> = m.edges.clone();
    number_pieces(&mut names, &pts, '~');
    names
}

/// The edge of a body with this name: exact, else the pieces of the same edge nearest `near`
/// (`Err` explains when there is none).
pub fn find_edge(b: &ModelBody, name: &str, near: Vec3) -> Result<(Vec3, bool), String> {
    locate_edge(b, name, near).map(|hit| (hit.point, hit.split))
}

/// Names of the edges nearest each point on a body (for storing references).
pub fn edge_names_at(b: &ModelBody, pts: &[Vec3]) -> Vec<String> {
    let m = b.mesh();
    let names = edge_names(b);
    pts.iter()
        .map(|p| {
            m.edges
                .iter()
                .enumerate()
                .map(|(i, e)| (e.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::MAX, f64::min), i))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .and_then(|(_, i)| names.get(i).cloned())
                .unwrap_or_default()
        })
        .collect()
}

/// Names of the faces holding each point (any body; empty when none does).
pub fn face_names_at(st: &ModelState, pts: &[Vec3]) -> Vec<String> {
    pts.iter()
        .map(|p| {
            for b in st.bodies.iter().filter(|b| !b.body.is_mesh()) {
                let tol = (b.body.size() * 1e-3).max(1e-3);
                let idx = FaceIndex::new(&b.mesh(), face_count(b));
                if let Some(f) = idx.face_at(*p, tol) {
                    return face_names(b).get(f).cloned().unwrap_or_default();
                }
            }
            String::new()
        })
        .collect()
}

/// Capture a face name from the recorded projection body only.
pub fn face_name_at_body(body: &ModelBody, point: Vec3) -> Option<String> {
    if body.body.is_mesh() || !point.is_finite() {
        return None;
    }
    let mesh = body.mesh();
    let count = face_count(body);
    if mesh.triangles.len() > 2_000_000 || count > 50_000 {
        return None;
    }
    let tolerance = (body.body.size() * 1e-3).max(1e-3);
    let index = FaceIndex::new(&mesh, count).face_at(point, tolerance)?;
    face_names(body).get(index).filter(|name| !name.is_empty()).cloned()
}

/// The names the faces of `imported` have in `part`, by body, then face (empty where not known):
/// `imported` are the bodies read back from the STEP text `part`'s solid bodies were written to
/// (a linked design), where they stand in it. Each takes the names of the solid of `part` that
/// most of its faces lie on, face by face: the same geometry, so a point well inside a face lies
/// on the face it was written from (and on a face of another solid only where two touch). Of
/// solids that hold as many (two alike in one place), the one the file names it after.
pub fn carried_face_names(part: &ModelState, imported: &[solidcraft_kernel::ImportedBody]) -> Vec<Vec<String>> {
    let solids: Vec<&ModelBody> = part.bodies.iter().filter(|b| !b.body.is_mesh()).collect();
    let indices: Vec<OnceLock<Option<FaceIndex>>> = solids.iter().map(|_| OnceLock::new()).collect();
    imported
        .iter()
        .map(|read| {
            let probe = ModelBody::new(String::new(), read.body.clone(), 0);
            let points: Vec<Option<Vec3>> = face_spots(&probe).into_iter().map(|s| s.map(|s| s.point)).collect();
            let best = solids
                .iter()
                .zip(&indices)
                .filter_map(|(b, index)| {
                    let tol = (b.body.size() * 2e-3).max(1e-3);
                    let near = b.outline_bounds().inflated(tol);
                    if !points.iter().flatten().any(|p| near.contains(*p)) {
                        return None;
                    }
                    // Like face_name_at_body, a body too large to index is left unnamed.
                    let index = index
                        .get_or_init(|| {
                            let mesh = b.mesh();
                            (mesh.triangles.len() <= 2_000_000 && face_count(b) <= 50_000).then(|| FaceIndex::new(&mesh, face_count(b)))
                        })
                        .as_ref()?;
                    let hits: Vec<Option<usize>> = points.iter().map(|p| p.and_then(|p| index.face_at(p, tol))).collect();
                    Some((hits.iter().flatten().count(), *b, hits))
                })
                .filter(|(n, ..)| *n > 0)
                .max_by_key(|(n, b, _)| (*n, b.name == read.name));
            let Some((_, b, hits)) = best else { return Vec::new() };
            let names = face_names(b);
            hits.into_iter().map(|h| h.and_then(|i| names.get(i).cloned()).unwrap_or_default()).collect()
        })
        .collect()
}

/// Where a face is, for a script: a point that lies on it and picks it, and the plane, cylinder,
/// cone, sphere or torus it is part of (when it is one).
#[derive(Clone, Copy, Debug)]
pub struct FaceSpot {
    pub point: Vec3,
    pub surface: Option<solidcraft_kernel::FaceSurface>,
}

/// The incentre and inradius of the roomiest triangle (greatest inradius) of each face of a mesh,
/// of face `only` alone if given.
fn roomiest(m: &Mesh, only: Option<usize>) -> Vec<Option<(f64, Vec3)>> {
    let mut best = vec![None; m.tri_face.iter().map(|f| *f as usize + 1).max().unwrap_or(0)];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let f = *f as usize;
        if only.is_some_and(|i| i != f) {
            continue;
        }
        let Some([a, b, c]) = m.tri(t) else { continue };
        // The incentre weighs each corner by the side opposite it; the inradius is area / half the perimeter.
        let (la, lb, lc) = (b.dist(c), c.dist(a), a.dist(b));
        let perimeter = la + lb + lc;
        let radius = (b - a).cross(c - a).len() / perimeter;
        let centre = (a * la + b * lb + c * lc) / perimeter;
        if let Some(slot) = best.get_mut(f)
            && radius.is_finite()
            && centre.is_finite()
            && slot.is_none_or(|(old, _)| radius > old)
        {
            *slot = Some((radius, centre));
        }
    }
    best
}

/// The spot of face `index` of `b` from its roomiest triangle: the triangle's incentre, so a point
/// that lies on the face however it is shaped (the centroid of a plate with a hole is in the hole,
/// that of a half cylinder inside the cylinder) and well away from its edges, so a pick there finds
/// this face. On a face of an analytic surface the point is moved onto the surface itself (a facet
/// of a curved face lies inside it by its sag), as far as that stays well inside the triangle.
fn spot(b: &ModelBody, index: usize, roomiest: Option<(f64, Vec3)>) -> Option<FaceSpot> {
    let (radius, centre) = roomiest?;
    let surface = solidcraft_kernel::face_surface(&b.body, index);
    let point = surface.and_then(|s| s.nearest(centre)).filter(|q| q.dist(centre) <= radius * 0.5).unwrap_or(centre);
    Some(FaceSpot { point, surface })
}

/// [`face_spot`] of every face of a body (`None`: the face has no triangles), in one pass.
pub fn face_spots(b: &ModelBody) -> Vec<Option<FaceSpot>> {
    roomiest(&b.mesh(), None).into_iter().enumerate().map(|(i, best)| spot(b, i, best)).collect()
}

/// Where face `index` of `b` is (`None`: it has no triangles).
pub fn face_spot(b: &ModelBody, index: usize) -> Option<FaceSpot> {
    spot(b, index, roomiest(&b.mesh(), Some(index)).get(index).copied().flatten())
}

/// A point on the face with this name, the name `model.faces` lists for it, on the body called
/// `body` or, without one, on any body (see [`locate_one`]): what a command that takes a face as
/// a point is given instead when the face is named.
pub fn named_face_point(st: &ModelState, body: Option<&str>, name: &str) -> Result<Vec3, String> {
    let hit = locate_one(st, body, name)?;
    let b = st.bodies.get(hit.body).ok_or_else(|| format!("no face is named `{name}`"))?;
    face_spot(b, hit.face).map(|spot| spot.point).ok_or_else(|| format!("face `{name}` has no surface to pick"))
}

/// The point on the face with this name nearest `p` (a split face: its nearest piece, with
/// `true` for "not exactly that face": split, or merged into another); `None` when no face has
/// the name.
pub fn point_on_face(st: &ModelState, name: &str, p: Vec3) -> Option<(Vec3, bool)> {
    locate_face(st, name, p).map(|hit| (hit.point, hit.split || hit.merged))
}

/// Resolve a legacy named face only on its recorded source body. Return the
/// selected face index as well as the closest point, which may lie on a shared
/// edge and therefore must not be used to pick the face a second time.
pub fn face_on_body(body: &ModelBody, name: &str, p: Vec3) -> Option<(usize, Vec3, bool)> {
    if body.body.is_mesh() {
        return None;
    }
    crate::name_lookup::locate_on(body, name, p).map(|hit| (hit.face, hit.point, hit.split || hit.merged))
}

#[cfg(test)]
mod tests {
    thread_local! {
        static WARM_JOBS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    pub(super) fn record_warm_jobs(n: usize) {
        WARM_JOBS.with(|jobs| jobs.set(jobs.get() + n));
    }

    fn take_warm_jobs() -> usize {
        WARM_JOBS.with(|jobs| jobs.replace(0))
    }

    fn lookup_bodies() -> ModelState {
        let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(4., 2., 1.)).unwrap();
        let mut st = ModelState::default();
        st.bodies = (0..13).map(|i| ModelBody::new(format!("Part{i}"), body.clone(), i + 1)).collect();
        st
    }

    fn same_hit(a: &FaceHit, b: &FaceHit) {
        assert_eq!((a.body, a.face, a.point), (b.body, b.face, b.point));
        assert_eq!((a.split, a.merged, a.verified, a.on_point, a.legacy), (b.split, b.merged, b.verified, b.on_point, b.legacy));
    }

    #[test]
    fn cached_face_lookups_schedule_no_name_warmup() {
        let st = lookup_bodies();
        for b in &st.bodies {
            let _ = face_names(b);
        }
        let name = face_names(&st.bodies[0])[0].clone();
        let picked = locate_face(&st, &name, Vec3::ZERO).unwrap();
        let unique = locate_one(&st, None, &name).unwrap();
        take_warm_jobs();
        // The two public lookup paths used by picked origins and named-face commands.
        for _ in 0..145 {
            same_hit(&locate_face(&st, &name, Vec3::ZERO).unwrap(), &picked);
            same_hit(&locate_one(&st, None, &name).unwrap(), &unique);
        }
        assert!(locate_face(&st, "absent:face:0", Vec3::ZERO).is_none());
        assert!(locate_one(&st, None, "absent:face:0").is_err());
        assert_eq!(take_warm_jobs(), 0, "cached names must not dispatch preparation work");
    }

    #[test]
    fn face_lookup_warms_only_cold_and_invalidated_names() {
        // Exercise both entry points as the first reader of a partly named model.
        for picked in [false, true] {
            let mut st = lookup_bodies();
            let name = face_names(&st.bodies[0])[0].clone();
            let lookup = |st: &ModelState, name: &str| {
                if picked { locate_face(st, name, Vec3::ZERO).unwrap() } else { locate_one(st, None, name).unwrap() }
            };
            take_warm_jobs();
            let hit = lookup(&st, &name);
            assert_eq!(take_warm_jobs(), 12, "only the other bodies need names");
            let snapshot = st.clone();
            same_hit(&lookup(&snapshot, &name), &hit);
            assert_eq!(take_warm_jobs(), 0, "snapshots share ready names");

            st.bodies[0].name = "Renamed".into();
            st.bodies[0].names = st.bodies[0].names.renamed();
            let renamed = lookup(&st, "Renamed:face:0");
            same_hit(&renamed, &hit);
            assert_eq!(take_warm_jobs(), 1, "a rename invalidates only its own names");
            assert!(locate_face(&st, &name, Vec3::ZERO).is_none());
            same_hit(&lookup(&snapshot, &name), &hit);
            assert_eq!(take_warm_jobs(), 0, "the retained snapshot stays ready");

            let source = st.bodies[1].clone();
            st.bodies[1].names = NameCell::same(&source);
            same_hit(&lookup(&st, "Renamed:face:0"), &renamed);
            assert_eq!(take_warm_jobs(), 1, "a lazy moved copy is prepared once");
            assert!(Arc::ptr_eq(&names(&st.bodies[1]), &names(&source)));
            same_hit(&lookup(&st, "Renamed:face:0"), &renamed);
            assert_eq!(take_warm_jobs(), 0);
        }
    }

    /// Pieces lying almost level along the sort axis must keep the order they arrived in, whatever
    /// the low bits of their centres do. The centre is the mean of a tessellated boundary, so those
    /// bits depend on where the loop's polyline starts and can differ between processes — which is
    /// how a name's number used to swap between runs (solidcraft#569).
    #[test]
    fn level_pieces_keep_their_input_order() {
        let piece = |dx: f64| vec![Vec3::new(10.0 + dx, 0.0, 0.0), Vec3::new(10.0 + dx, 0.0, 5.0)];
        for (first, second) in [(0.0, 1e-12), (1e-12, 0.0)] {
            let mut names = vec!["f".to_string(), "f".to_string()];
            let pts = vec![piece(first), piece(second)];
            number_pieces(&mut names, &pts, '#');
            assert_eq!(names, vec!["f#1".to_string(), "f#2".to_string()], "input order not kept for {first} then {second}");
        }
    }

    /// Three pieces spaced just under the tolerance apart: `a` and `b` in one bucket and `b` and `c`
    /// in the next, so a tolerance comparison would call `a≈b` and `b≈c` while `a<c` — the
    /// non-transitivity that leaves `sort_by` without a total order to work with. The key must order
    /// them by bucket, and within a bucket by the order they arrived in.
    #[test]
    fn pieces_just_under_the_tolerance_apart_still_have_a_total_order() {
        let piece = |dx: f64| vec![Vec3::new(10.0 + dx, 0.0, 0.0), Vec3::new(10.0 + dx, 0.0, 5.0)];
        // The grid is the spread of the centres floored at 1e-9; these centres span under 1.0, so it
        // is 1e-9 and the three offsets below sit in buckets 0, 0 and 1.
        let grid = 1e-9;
        let step = grid * 0.4;
        for ((a, b, c), want) in [
            ((0.0, step, 2.0 * step), ["f#1", "f#2", "f#3"]),
            ((2.0 * step, step, 0.0), ["f#3", "f#1", "f#2"]),
            ((0.0, 2.0 * step, step), ["f#1", "f#3", "f#2"]),
            ((step, 0.0, 2.0 * step), ["f#1", "f#2", "f#3"]),
        ] {
            let mut names = vec!["f".to_string(); 3];
            let pts = [a, b, c].iter().map(|d| piece(*d)).collect::<Vec<_>>();
            number_pieces(&mut names, &pts, '#');
            assert_eq!(names, want, "wrong order for offsets {a} {b} {c}");
        }
    }

    use super::*;

    /// A cone's tip repeats its apex vertex, so a triangle can have a zero-length edge. The nearest
    /// point on it must still be a real point (it used to be NaN, which broke picking the tip).
    #[test]
    fn closest_point_on_a_degenerate_triangle_is_finite() {
        let (a, c) = (Vec3::new(0., 0., 0.), Vec3::new(2., 0., 0.));
        for (b, c) in [(a, c), (c, c), (a, a)] {
            for p in [Vec3::new(1., 1., 0.), Vec3::new(-1., 0.5, 0.), Vec3::new(3., -2., 1.), a] {
                let q = closest(p, a, b, c);
                assert!(q.x.is_finite() && q.y.is_finite() && q.z.is_finite(), "{a:?} {b:?} {c:?} {p:?} -> {q:?}");
            }
        }
        assert_eq!(closest(Vec3::new(1., 1., 0.), a, a, c), Vec3::new(1., 0., 0.));
    }

    #[test]
    fn merged_face_votes_collectively_for_pieces_of_one_source() {
        let body = ModelBody::new("Input".into(), solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(4., 1., 1.)).unwrap(), 1);
        let mut mesh = Mesh::default();
        let mut points = Vec::new();
        for i in 0..3u32 {
            let x = f64::from(i);
            mesh.positions.extend([Vec3::new(x, 0., 0.), Vec3::new(x + 0.8, 0., 0.), Vec3::new(x, 0.8, 0.)]);
            mesh.triangles.push([i * 3, i * 3 + 1, i * 3 + 2]);
            mesh.tri_face.push(i);
            points.push(Vec3::new(x + 0.2, 0.2, 0.));
        }
        let pieces = Arc::new(vec!["F1:side:wall#1".into(), "F1:side:wall#2".into(), "F1:side:wall#3".into()]);
        assert_eq!(vote(&points, &[(&body, FaceIndex::new(&mesh, 3), pieces)], 1e-8, None).as_deref(), Some("F1:side:wall"));
        let distinct = Arc::new(vec!["F1:side:wall#1".into(), "F2:side:wall#2".into(), "F3:side:wall#3".into()]);
        let inputs = [(&body, FaceIndex::new(&mesh, 3), distinct)];
        assert_eq!(vote(&points, &inputs, 1e-8, None), None);
        assert_eq!(vote(&points[..2], &inputs, 1e-8, None), None, "equal different-source votes must not pick a lexicographic winner");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn release_fixture(count: usize) -> (Arc<ModelState>, Vec<std::sync::Weak<ModelState>>) {
        let body = ModelBody::new("Original release fixture".into(), solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(1., 1., 1.)).unwrap(), 1);
        let feature: Feature = serde_json::from_value(serde_json::json!({
            "id":1,"name":"Original naming boundary","type":"body_transfer","bodies":[],
            "from":0,"to":1,"transform":crate::IDENTITY
        }))
        .unwrap();
        let mut state = Arc::new(ModelState::default());
        let mut weak = vec![Arc::downgrade(&state)];
        for depth in 0..count {
            let mut next = body.clone();
            next.names =
                NameCell::new(Some(Arc::new(NamingOrigin { feature: feature.clone(), before: state, depth, mats: Vec::new(), sheets: Vec::new() })));
            let mut next_state = ModelState::default();
            next_state.bodies.push(next);
            state = Arc::new(next_state);
            weak.push(Arc::downgrade(&state));
        }
        (state, weak)
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn deep_naming_ancestry_releases_without_timeline_sized_call_stack() {
        let (state, weak) = release_fixture(20_000);
        let held = state.clone();
        drop(state);
        assert!(weak.iter().all(|node| node.upgrade().is_some()), "a live shared model retains its complete ancestry");
        std::thread::Builder::new().stack_size(256 * 1024).spawn(move || drop(held)).unwrap().join().unwrap();
        assert!(weak.iter().all(|node| node.upgrade().is_none()), "no ancestor or geometry is leaked after the last owner closes");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn long_placed_copy_chain_reclaims_every_source_on_a_small_stack() {
        let mut body = ModelBody::new("Original copied fixture".into(), solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(1., 1., 1.)).unwrap(), 1);
        let mut weak = Vec::new();
        for _ in 0..20_000 {
            let names = NameCell::same(&body);
            weak.push(Arc::downgrade(names.copy_of.as_ref().unwrap()));
            body.names = names;
        }
        std::thread::Builder::new().stack_size(256 * 1024).spawn(move || drop(body)).unwrap().join().unwrap();
        assert!(weak.iter().all(|node| node.upgrade().is_none()));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn concurrent_final_naming_owners_do_not_leak_or_mutate_shared_ancestors() {
        let (state, weak) = release_fixture(8_000);
        let owner = NameCell::same(&state.bodies[0]);
        let other = owner.clone();
        drop(state);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let gate = barrier.clone();
        let first = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                gate.wait();
                drop(owner);
            })
            .unwrap();
        let second = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                barrier.wait();
                drop(other);
            })
            .unwrap();
        first.join().unwrap();
        second.join().unwrap();
        assert!(weak.iter().all(|node| node.upgrade().is_none()));
    }

    /// A face is named after the nearest sketch curve; skipping curves by their boxes finds the
    /// same curve as measuring every one, the first of equally near ones included.
    #[test]
    fn the_curve_index_finds_the_curve_a_full_scan_finds() {
        use solidcraft_sketch::{Sketch, find_profiles, solve};
        let mut sketch = Sketch::new();
        for i in 0..6 {
            for j in 0..4 {
                sketch.add_circle(Vec2::new(10. * f64::from(i), 10. * f64::from(j)), 2.5, None, Some(&format!("c{i}_{j}"))).unwrap();
            }
        }
        sketch.add_line(Vec2::new(-5., -5.), Vec2::new(55., -5.), None, None, Some("bottom")).unwrap();
        sketch.add_arc(Vec2::new(25., 15.), Vec2::new(45., 15.), Vec2::new(5., 15.), [None; 3], Some("arc")).unwrap();
        let hidden = sketch.add_line(Vec2::new(-5., 12.), Vec2::new(60., 12.), None, None, Some("guide")).unwrap();
        sketch.curves[hidden].construction = true;
        let report = solve(&mut sketch);
        let profiles = find_profiles(&sketch);
        let ss = crate::SolvedSketch {
            feature: 1,
            name: "S".into(),
            plane: solidcraft_geom::Plane::XY,
            sketch,
            report,
            profiles,
            profiles_3d: Vec::new(),
            ends: Default::default(),
        };
        let full_scan = |q: Vec2| {
            let mut best: Option<(f64, String)> = None;
            for (ci, c) in ss.sketch.curves.iter().enumerate().filter(|(_, c)| !c.construction) {
                for s in ss.sketch.segs(ci) {
                    let d = polyline_dist(&s.polyline(1e-3), q);
                    if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                        best = Some((d, c.id.clone()));
                    }
                }
            }
            best
        };
        let curves = Curves::of(&ss);
        // A grid of points over and around the sketch; x = 5, 15, ... lie midway between circles.
        for x in -16..=124 {
            for y in -16..=84 {
                let q = Vec2::new(f64::from(x) * 0.5, f64::from(y) * 0.5);
                assert_eq!(curves.nearest(q), full_scan(q), "at {q:?}");
            }
        }
        assert_eq!(curves.nearest(Vec2::new(5., 0.)).map(|(_, id)| id).as_deref(), Some("c0_0"), "a tie keeps the first curve");
        assert!(curves.segs.iter().all(|(id, ..)| id != "guide"), "construction curves name nothing");
    }
}
