//! Bounded desktop preparation of independent imported bodies, collected in file order.
use std::sync::Arc;

use super::{Body, Exchange, IDENTITY, KernelError, M4, Units};

pub(super) struct Job {
    pub source: Source,
    /// Where the body goes (its instance's placement in the assembly).
    pub placement: M4,
    pub check_mesh: bool,
}

/// What a job places: a solid item to read from the file (and orient), or a part an earlier
/// instance of the same item already read and oriented.
pub(super) enum Source {
    Item { item: u64, units: Units },
    Oriented(Arc<Part>),
}

/// A solid item read and oriented where the file defines it, placed by each of its instances.
#[derive(Clone)]
pub(super) struct Part {
    pub body: Body,
    /// The shells are closed.
    pub closed: bool,
    /// Faces the file had.
    pub file_faces: usize,
    /// The file face of each kernel face.
    pub face_origin: Vec<u64>,
    /// No B-rep could be made of the whole solid, and this is why: the body is a surface body of
    /// the faces that read ([`surface_part`]).
    pub refused: Option<String>,
    /// SMLib edges the export split because only sheet faces use them more than twice (each once).
    pub split_edges: usize,
}

/// A prepared instance.
pub(super) struct Made {
    /// The part, placed.
    pub body: Body,
    /// Faces the mesher missed (counted when the job asked).
    pub missing: usize,
    pub part: Arc<Part>,
    /// This job read the item: what the read repaired and the faces it left out.
    pub read: Option<Vec<Note>>,
}

/// What reading an item noted.
pub(super) enum Note {
    /// Repaired on the way (the reader's notes).
    Repaired(String),
    /// A face left out, with the reason.
    LeftOut(String),
}

/// Why a job made no body: the item could not be read (`read`), or its body not made.
pub(super) struct Failure {
    pub read: bool,
    pub msg: String,
}

type Prepared = std::result::Result<Made, Failure>;

pub(super) fn workers() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::available_parallelism().map_or(1, |n| n.get().min(8))
    }
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
}

pub(super) fn prepare(ex: &Exchange, job: &Job) -> Prepared {
    let _span = solidcraft_profile::span("import", "step.body");
    let (part, read) = match &job.source {
        Source::Oriented(part) => (part.clone(), None),
        Source::Item { item, units } => {
            // AP242 triangles (#3924): a mesh body, as STL and 3MF bring in.
            if ex.get(*item).is_some_and(super::tessellated::is_item) {
                let body = super::tessellated::read(ex, *item, units.units.len).map_err(|msg| Failure { read: true, msg })?;
                let closed = body.is_closed_mesh();
                (Arc::new(Part { body, closed, file_faces: 0, face_origin: Vec::new(), refused: None, split_edges: 0 }), Some(Vec::new()))
            } else {
                let (part, read) = smlib_part(ex, *item, *units).or_else(|refused| surface_part(ex, *item, *units, refused))?;
                (Arc::new(part), Some(read))
            }
        }
    };
    let body = solidcraft_profile::scope("import", "step.place", || placed(&part.body, &job.placement))
        .map_err(|e| Failure { read: false, msg: e.to_string() })?;
    // Counted on the part (a placement meshes the same faces). A part not moved is the body
    // itself: it keeps the display plan its check made, which its display then takes.
    let (body, missing) = match job.check_mesh {
        false => (body, 0),
        true if job.placement == IDENTITY => {
            let body = body.keeping_plans();
            let missing = solidcraft_profile::scope("import", "step.mesh_check", || body.unmeshed_display_faces());
            (body, missing)
        }
        true => {
            let missing = solidcraft_profile::scope("import", "step.mesh_check", || part.body.unmeshed_faces());
            (body, missing)
        }
    };
    Ok(Made { body, missing, part, read })
}

/// What [`heal`] did with a part.
pub(super) enum Healed {
    /// Healed: the part with its body and file faces replaced, what the import says of it, and
    /// SMLib's check of the body kept ([`crate::ToleranceHeal::check`]: `None` when it passes),
    /// and for each face of it the face of the part it was ([`crate::ToleranceHeal::face_source`]).
    Kept(Box<Part>, String, Option<crate::CheckFailure>, Vec<usize>),
    /// Nothing the heal could prove, or a result whose check is not better: the part as it was.
    NotKept,
    /// The heal could not run (the part as it was): never a reason to change it.
    CouldNotRun(KernelError),
    /// The token over the import's work stopped it (the part as it was).
    Stopped,
}

/// A part whose check fails, healed of its tolerance-scale defects
/// ([`crate::heal_tolerance_defects`]) when that can be proved and leaves the check failing
/// fewer tests: a new part, with its body and its faces' file faces (the heal only removes
/// faces) replaced. Otherwise [`Healed`] says why not. `own` is SMLib's check of `part.body`
/// when the import already ran it ([`crate::tolerance_heal::heal_given_check`]; `None`: the heal
/// runs it when it needs it).
pub(super) fn heal(part: &Part, item: u64, own: Option<&crate::CheckFailure>) -> Healed {
    let (body, heal) = match crate::tolerance_heal::heal_given_check(&part.body, own) {
        Ok(Some(healed)) => healed,
        Ok(None) => return Healed::NotKept,
        Err(KernelError::Heal(crate::HealFailure::Stopped)) => return Healed::Stopped,
        Err(e) => return Healed::CouldNotRun(e),
    };
    let Some(face_origin) = heal.face_source.iter().map(|&f| part.face_origin.get(f).copied()).collect::<Option<Vec<u64>>>() else {
        return Healed::CouldNotRun(KernelError::Heal(crate::HealFailure::CouldNotRun(format!(
            "the heal of item #{item} named a face the part does not have"
        ))));
    };
    let healed =
        Part { body, closed: part.closed, file_faces: part.file_faces, face_origin, refused: part.refused.clone(), split_edges: part.split_edges };
    Healed::Kept(Box::new(healed), format!("healed SMLib's check failures: {}", heal.summary()), heal.check, heal.face_source)
}

/// The part's body where its instance goes: the very same body at the identity (it keeps the
/// SMLib body the read made), else its record moved there.
pub(super) fn placed(body: &Body, m: &M4) -> crate::Result<Body> {
    if *m == IDENTITY {
        return Ok(body.clone());
    }
    Ok(crate::ops::transform_matrix(body, *m)?.without_history())
}

/// A solid item read with the STEP reader into its schema 4 record, then into SMLib as given,
/// and brought back as a body: the part SMLib made of the file. A solid the reader or SMLib
/// refuses fails with the reason ([`surface_part`] then imports what it can of it).
pub(super) fn smlib_part(ex: &Exchange, item: u64, units: Units) -> std::result::Result<(Part, Vec<Note>), Failure> {
    let solid = solidcraft_profile::scope("import", "step.read_solid", || {
        solidcore_step::topology::read_solid_with(ex, item, units, &solidcraft_smlib::pcurves::SmlibImportTrims)
    })
    .map_err(|msg| Failure { read: true, msg })?;
    let part = solid_part(&solid)?;
    Ok((part, solid.notes.into_iter().map(Note::Repaired).collect()))
}

/// The part SMLib makes of a solid (or sheet) record the reader made.
///
/// SMLib holds both sides of a sheet's faces and gives each back facing its surface's natural
/// normal, so a sheet whose file uses some faces the other way is given to SMLib with those faces
/// turned over (exactly, on the record) and they are turned back on SMLib's body: the body faces
/// as the file does, and each of its faces is known to be the record's face it was.
pub(super) fn solid_part(solid: &solidcore_step::topology::Solid) -> std::result::Result<Part, Failure> {
    record_part(&solid.brep, &solid.face_origin, solid.sheet)
}

/// [`solid_part`] of a record given as plain data: the record, the file face each of its faces
/// is, and whether it is a sheet.
pub(super) fn record_part(record: &solidcore_record::brep::Brep, face_origin: &[u64], sheet: bool) -> std::result::Result<Part, Failure> {
    let failed = |e: KernelError| Failure { read: false, msg: e.to_string().trim_start_matches("the operation failed: ").to_string() };
    let r = record;
    let mut reversed = vec![false; r.faces.len()];
    if sheet {
        for s in &r.shells {
            if let solidcore_record::brep::Shell::Faces { faces, .. } = s {
                for f in faces.iter().filter(|f| !f.forward) {
                    if let Some(slot) = reversed.get_mut(f.face as usize) {
                        *slot = true;
                    }
                }
            }
        }
    }
    let (body, of_record, split_edges) = if reversed.iter().any(|r| *r) {
        let turn = |body: &Body, by: &dyn Fn(usize) -> bool| -> crate::Result<(Body, Vec<usize>)> {
            let picked: Vec<(usize, bool)> = (0..body.face_count()).map(|f| (f, by(f))).collect();
            let turned = crate::surfaces::taken(body, body.record(), &picked)?;
            let from = kept_faces(&turned).ok_or_else(|| KernelError::Failed("a face turned over from no one face".into()))?;
            Ok((turned, from))
        };
        let as_read = Body::from_record(r.clone()).map_err(failed)?;
        // `natural`'s face j is the record's `to_record[j]`; SMLib's body's face k is `natural`'s
        // `of_natural[k]`; the body turned back's face i is SMLib's `back[i]`.
        let (natural, to_record) = turn(&as_read, &|f| reversed.get(f).copied().unwrap_or(false)).map_err(failed)?;
        // SMLib names a face it refuses by its index in the record it was given, `natural`'s.
        let natural_origin: Vec<u64> = to_record.iter().map(|f| face_origin.get(*f).copied()).collect::<Option<_>>().unwrap_or_default();
        let (smlib, of_natural, split) = smlib_body(natural.record()).map_err(|f| Failure { msg: name_record_face(&f.msg, &natural_origin), ..f })?;
        let record_of = |k: usize| of_natural.get(k).and_then(|j| to_record.get(*j)).copied();
        let (back, from) = turn(&smlib, &|k| record_of(k).and_then(|f| reversed.get(f)).copied().unwrap_or(false)).map_err(failed)?;
        let of_record = from.iter().map(|k| record_of(*k)).collect::<Option<Vec<usize>>>().ok_or_else(renumbered)?;
        (back.without_history(), of_record, split)
    } else {
        let (body, of_record, split) = smlib_body(r).map_err(|f| Failure { msg: name_record_face(&f.msg, face_origin), ..f })?;
        (body, of_record, split)
    };
    // The file's faces are the record's, however many faces SMLib's body has.
    let file_faces = face_origin.len();
    let face_origin = of_record.iter().map(|f| face_origin.get(*f).copied()).collect::<Option<Vec<u64>>>().ok_or_else(renumbered)?;
    Ok(Part { body, closed: !sheet, file_faces, face_origin, refused: None, split_edges })
}

fn renumbered() -> Failure {
    Failure { read: false, msg: "not supported yet: SMLib renumbered the solid's faces".into() }
}

/// The body SMLib makes of `record` as given, and the record's face each of its faces is.
fn smlib_body(record: &solidcore_record::brep::Brep) -> std::result::Result<(Body, Vec<usize>, usize), Failure> {
    let failed = |e: KernelError| Failure { read: false, msg: e.to_string().trim_start_matches("the operation failed: ").to_string() };
    let (brep, smlib_order) = crate::native::smlib_record_ordered(record).map_err(failed)?;
    let (canonical, body_order, prepared, exported) = brep.to_native_prepared().map_err(|e| failed(KernelError::Failed(e.to_string())))?;
    let body = Body::from_smlib_record(canonical).map_err(failed)?;
    if let Some(prepared) = prepared {
        // Kept as the body's SMLib form, as its first use would make it.
        body.smlib.keep(prepared, body.smlib.releases());
    }
    // SMLib's face i is the record's `smlib_order` face and the body's `body_order` face (an
    // empty order is the same one).
    // SMLib's export may number the edges and vertices otherwise than the record (a surface model
    // whose open shell uses faces against their surfaces beside closed shells: SMLib walks those
    // faces as their surfaces face). An import still lists SMLib's faces in the record's order;
    // that is taken where each face is checked to be the record's face it would be.
    let smlib_order = match smlib_order {
        Some(order) => order,
        None if faces_in_record_order(record, body.record(), &body_order.faces) => Default::default(),
        None => return Err(renumbered()),
    };
    let at = |order: &[usize], i: usize| if order.is_empty() { Some(i) } else { order.get(i).copied() };
    let faces = brep.counts().map_err(|e| failed(KernelError::Failed(format!("SMLib: {e}"))))?[0];
    let mut of_record = vec![usize::MAX; body.face_count()];
    for i in 0..faces {
        match (at(&body_order.faces, i).and_then(|b| of_record.get_mut(b)), at(&smlib_order.faces, i).filter(|r| *r < record.faces.len())) {
            (Some(slot), Some(r)) => *slot = r,
            _ => return Err(renumbered()),
        }
    }
    if of_record.contains(&usize::MAX) {
        return Err(renumbered());
    }
    Ok((body, of_record, exported.split_edges))
}

/// Whether SMLib's face `i` of a body made from `record` (the body's face `body_order[i]`, or `i`
/// for an empty order) is the record's face `i`, face by face: the same kind of surface, the same
/// loops of the same lengths, each loop through the same corners (within the vertices' and the
/// records' tolerances, whichever way and from wherever SMLib walks it).
fn faces_in_record_order(record: &solidcore_record::brep::Brep, body: &solidcore_record::brep::Brep, body_order: &[usize]) -> bool {
    use solidcore_record::brep::{Brep, Loop};
    if record.faces.len() != body.faces.len() || !(body_order.is_empty() || body_order.len() == body.faces.len()) {
        return false;
    }
    // Each loop's corners: the start of each of its edges as the loop walks them, with its tolerance.
    let corners = |b: &Brep, l: &Loop| -> Option<Vec<([f64; 3], f64)>> {
        let vertex = |v: u32| b.vertices.get(usize::try_from(v).ok()?).map(|x| (x.point, x.tolerance));
        match l {
            Loop::Vertex { vertex: v } => Some(vec![vertex(*v)?]),
            Loop::Edges { edges } => edges
                .iter()
                .map(|e| {
                    let edge = b.edges.get(usize::try_from(e.edge).ok()?)?;
                    vertex(if e.forward { edge.vertices[0] } else { edge.vertices[1] })
                })
                .collect(),
        }
    };
    let slack = record.tolerance + body.tolerance;
    (0..record.faces.len()).all(|i| {
        let at = if body_order.is_empty() { Some(i) } else { body_order.get(i).copied() };
        let (Some(a), Some(b)) = (record.faces.get(i), at.and_then(|k| body.faces.get(k))) else { return false };
        if std::mem::discriminant(&a.surface) != std::mem::discriminant(&b.surface) || a.loops.len() != b.loops.len() {
            return false;
        }
        a.loops.iter().zip(&b.loops).all(|(x, y)| {
            let (Some(p), Some(q)) = (corners(record, x), corners(body, y)) else { return false };
            p.len() == q.len()
                && p.iter().all(|(u, tu)| q.iter().any(|(w, tw)| (0..3).map(|k| (u[k] - w[k]).powi(2)).sum::<f64>().sqrt() <= tu + tw + slack))
        })
    })
}

/// The face of its one operand each face of `body` is, from its history.
fn kept_faces(body: &Body) -> Option<Vec<usize>> {
    let h = body.history()?;
    let from: Vec<usize> = h
        .faces
        .iter()
        .map(|f| match f.origins.as_slice() {
            [crate::history::FaceOrigin::Face { operand: 0, face }] => Some(*face),
            _ => None,
        })
        .collect::<Option<_>>()?;
    (from.len() == body.face_count()).then_some(from)
}

/// Most reads of a refused solid without faces, each leaving out the face the refusal before names.
const MAX_LEFT_OUT_ROUNDS: usize = 8;

/// A solid the reader or SMLib refuses, imported as an open surface body of its exact faces, the
/// faces that make it refused left out and named in the notes; `refused` says why the solid was.
/// Each face is read on its own first ([`solidcore_step::topology::read_faces_with`]); then:
/// 1. the solid is read again without the faces that cannot be read alone and the face the
///    refusal names, as one sheet whose faces share their edges as the file has them, and made
///    by SMLib; a refusal of that naming another face of the solid leaves it out too (up to
///    [`MAX_LEFT_OUT_ROUNDS`] reads). A face is only ever left out where a refusal names it.
///    A shell the file leaves open is read this way with no face left out, when sewing its faces
///    (step 2) does not close it.
/// 2. Where a refusal names no face of the solid: the faces that read alone, stitched by SMLib
///    into one surface body (their edges sewn at ten times their tolerance).
///
/// Where SMLib cannot stitch them either, or there is no face to import, the solid stays refused
/// with the reason it was: a B-rep SMLib refuses is never kept as a mesh of its faces.
fn surface_part(ex: &Exchange, item: u64, units: Units, refused: Failure) -> std::result::Result<(Part, Vec<Note>), Failure> {
    let _span = solidcraft_profile::span("import", "step.refused_surfaces");
    let trims = &solidcraft_smlib::pcurves::SmlibImportTrims;
    let Ok(faces) = solidcore_step::topology::read_faces_with(ex, item, units, trims) else { return Err(refused) };
    let of_solid: std::collections::HashSet<u64> = faces.iter().map(|f| f.face).collect();
    // The faces left out, each with the refusal that names it, in the order they were found.
    let mut left_out: Vec<(u64, String)> = faces.iter().filter_map(|f| f.record.as_ref().err().map(|m| (f.face, m.clone()))).collect();
    // The faces that read alone, each a one-face surface body.
    let mut notes = Vec::new();
    let mut pieces = Vec::with_capacity(faces.len());
    for f in &faces {
        // A face the solid's shells use more than once (an outer shell and its voids written
        // with one face) is one piece, once.
        if pieces.iter().any(|(g, _)| *g == f.face) {
            notes.push(Note::LeftOut(format!("left out: face #{}: a further use of it by another shell of the solid", f.face)));
            continue;
        }
        match f.record.clone().map_err(KernelError::Failed).and_then(Body::from_record) {
            Ok(b) => pieces.push((f.face, b)),
            Err(e) => notes.push(Note::LeftOut(format!("left out: {}", e.to_string().trim_start_matches("the operation failed: ")))),
        }
    }
    let mut why = refused.msg.clone();
    let mut rounds = 0;
    loop {
        // The face the last refusal names, if it is one of the solid's still in, is left out too.
        let named = solidcore_step::topology::named_face(&why).filter(|f| of_solid.contains(f) && !left_out.iter().any(|(g, _)| g == f));
        match named {
            Some(f) => left_out.push((f, why.clone())),
            None if rounds > 0 => break,
            None => {}
        }
        // A shell the file leaves open (an edge only one face uses) is one sheet of all its faces,
        // which share the edges they do: read without leaving any out.
        let open_shell = rounds == 0 && left_out.is_empty() && refused.msg.contains("shell is not closed");
        // Its faces are sewn first, at ten times their tolerance: where that closes the shell it is
        // the solid, and only a shell the sewing leaves open is read as the sheet the file has.
        if open_shell
            && let Ok(part) = stitched_part(&pieces, faces.len(), &refused.msg)
            && part.closed
        {
            return Ok((part, notes));
        }
        if (left_out.is_empty() && !open_shell) || left_out.len() >= faces.len() || rounds >= MAX_LEFT_OUT_ROUNDS {
            break;
        }
        rounds += 1;
        let skip: std::collections::HashSet<u64> = left_out.iter().map(|(f, _)| *f).collect();
        let read = solidcraft_profile::scope("import", "step.read_without", || {
            if open_shell {
                solidcore_step::topology::read_solid_as_sheets(ex, item, units, trims)
            } else {
                solidcore_step::topology::read_solid_without(ex, item, units, trims, &skip)
            }
        });
        match read.map_err(|msg| Failure { read: true, msg }).and_then(|solid| {
            let part = solid_part(&solid)?;
            Ok((part, solid.notes.into_iter().map(Note::Repaired).collect::<Vec<_>>()))
        }) {
            Ok((mut part, mut notes)) => {
                part.file_faces = faces.len();
                part.refused = Some(refused.msg);
                notes.extend(left_out.iter().map(|(_, m)| Note::LeftOut(format!("left out: {m}"))));
                return Ok((part, notes));
            }
            Err(e) => {
                log::debug!(target: "step", "item #{item}: read without {} faces, refused: {}", skip.len(), e.msg);
                why = e.msg;
            }
        }
    }
    // A solid of one face that cannot be made has nothing else to give: the refusal says it is the
    // only face, which face, and why (an edge's distance from the surface, a vertex's from its
    // curve), never only that the import failed.
    let distinct: std::collections::HashSet<u64> = faces.iter().map(|f| f.face).collect();
    if pieces.is_empty()
        && distinct.len() == 1
        && let Some(f) = faces.first()
    {
        let why = left_out.iter().find(|(g, _)| *g == f.face).map_or(refused.msg.as_str(), |(_, m)| m.as_str());
        let why = why.split_once("first: ").map_or(why, |(_, rest)| rest);
        return Err(Failure { msg: format!("the solid's only face, #{}, cannot be made: {why}", f.face), ..refused });
    }
    // Where SMLib cannot stitch them either, the solid is refused with the reason it was.
    let part = stitched_part(&pieces, faces.len(), &refused.msg).map_err(|e| {
        log::debug!(target: "step", "item #{item}: its faces not stitched ({e})");
        refused
    })?;
    Ok((part, notes))
}

/// A refusal of SMLib's that names a face of the record it was given by its index there
/// ("flat arrays: face 12: …"), with that face named as the file's entity too ("face #1167"), so
/// the face can be left out and the import say which it was.
pub(super) fn name_record_face(msg: &str, face_origin: &[u64]) -> String {
    let Some(at) = msg.find("flat arrays: face ") else { return msg.to_string() };
    let Some(rest) = msg.get(at + "flat arrays: face ".len()..) else { return msg.to_string() };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    match digits.parse::<usize>().ok().and_then(|i| face_origin.get(i)) {
        Some(step) => format!("face #{step}: {msg}"),
        None => msg.to_string(),
    }
}

/// The one-face surface bodies `pieces` (each with its file face) stitched by SMLib into one
/// surface body, where their edges coincide within ten times the pieces' tolerance.
fn stitched_part(pieces: &[(u64, Body)], file_faces: usize, refused: &str) -> crate::Result<Part> {
    if pieces.is_empty() {
        return Err(KernelError::Failed("no face read".into()));
    }
    // One face read alone is the sheet: nothing to stitch it to.
    if let [(face, body)] = pieces {
        return Ok(Part {
            body: body.clone(),
            closed: false,
            file_faces,
            face_origin: vec![*face],
            refused: Some(refused.to_string()),
            split_edges: 0,
        });
    }
    let tol = 10.0 * pieces.iter().map(|(_, b)| b.record().tolerance).fold(0.0, f64::max);
    let bodies: Vec<&Body> = pieces.iter().map(|(_, b)| b).collect();
    let body = crate::surfaces::stitch(&bodies, tol)?;
    // Each stitched face is the one face of the piece it was sewn from.
    let history = body.history().ok_or_else(|| KernelError::Failed("the stitch kept no history".into()))?;
    let face_origin = history
        .faces
        .iter()
        .map(|f| match f.origins.as_slice() {
            [crate::history::FaceOrigin::Face { operand, .. }] => pieces.get(*operand).map(|(face, _)| *face),
            _ => None,
        })
        .collect::<Option<Vec<u64>>>()
        .filter(|o| o.len() == body.face_count())
        .ok_or_else(|| KernelError::Failed("a stitched face from no one face".into()))?;
    let closed = !body.is_surface();
    Ok(Part { body, closed, file_faces, face_origin, refused: Some(refused.to_string()), split_edges: 0 })
}

/// Prepare `jobs`, the results in the jobs' order. Solids still to read and orient (the slow
/// part) are shared out between up to `workers` threads, each taking the next job when it is
/// done with one, so one large part does not hold up the rest.
pub(super) fn bodies(ex: &Exchange, jobs: &[Job], workers: usize) -> Vec<Prepared> {
    let to_orient = jobs.iter().filter(|j| matches!(j.source, Source::Item { .. })).count();
    if to_orient > 1 && workers > 1 {
        return crate::par::map_on_threads(jobs, workers.min(8).min(to_orient), |job| prepare(ex, job))
            .into_iter()
            .map(|r| r.unwrap_or_else(|| Err(Failure { read: false, msg: "STEP body worker panicked".into() })))
            .collect();
    }
    jobs.iter().map(|job| prepare(ex, job)).collect()
}
