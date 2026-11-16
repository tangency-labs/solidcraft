//! Bounded desktop preparation of independent imported bodies, collected in file order.
use std::sync::Arc;

use super::{Body, Exchange, IDENTITY, KernelError, M4, Units};

pub(super) struct Job {
    pub source: Source,
    /// Where the body goes (its instance's placement in the assembly).
    pub placement: M4,
    pub check_mesh: bool,
    /// Check the body placed, in SMLib ([`PlacedCheck`]): the first instance of its part under
    /// its placement's class ([`super::check_class`]).
    pub check: bool,
}

/// What a job places: a solid item to read from the file (and orient), or a part an earlier
/// instance of the same item already read and oriented.
pub(super) enum Source {
    Item { item: u64, units: Units },
    Oriented(Arc<Part>),
}

/// A solid item read and oriented where the file defines it, placed by each of its instances.
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
    /// SMLib's check of the body placed, when the job asked for it.
    pub check: Option<PlacedCheck>,
}

/// SMLib's check of a placed body, as an operation on it takes it
/// ([`crate::validation::failing_check`]): operations that check their results refuse them
/// while it fails or cannot run.
pub(super) enum PlacedCheck {
    /// Not run: the part is made of surfaces (no B-rep could be made of the whole solid).
    Skipped,
    /// It passes (or SMLib refuses the body outright, which [`Body::smlib_refusal`] says).
    Passes,
    Fails(crate::CheckFailure),
    CouldNotRun(String),
    /// The token over the import's work stopped it: the import is stopped.
    Stopped,
}

/// What reading an item noted.
pub(super) enum Note {
    /// Repaired on the way (the reader's notes).
    Repaired(String),
    /// A face left out, with the reason.
    LeftOut(String),
    /// The heal of the part's check failures failed inside the kernel, with why: the part
    /// stands as read, and still needs repair.
    HealFailed(String),
}

/// Why a job made no body: the item could not be read (`read`), or its body not made.
pub(super) struct Failure {
    pub read: bool,
    pub msg: String,
}

pub(super) type Prepared = std::result::Result<Made, Failure>;

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
    // This job's body placed and its check, when the read already made them deciding the heal.
    let mut done: Option<(Body, PlacedCheck)> = None;
    let (part, read) = match &job.source {
        Source::Oriented(part) => (part.clone(), None),
        Source::Item { item, units } => {
            let (mut part, mut read) = smlib_part(ex, *item, *units).or_else(|refused| surface_part(ex, *item, *units, refused))?;
            if part.refused.is_none() {
                done = decide_heal(&mut part, &mut read, &job.placement, *item)?;
            }
            (Arc::new(part), Some(read))
        }
    };
    let (body, check) = match done {
        Some((body, check)) => (body, Some(check)),
        None => (place(&part.body, &job.placement)?, None),
    };
    // Counted on the part (a placement meshes the same faces).
    let missing = if job.check_mesh { solidcraft_profile::scope("import", "step.mesh_check", || part.body.unmeshed_faces()) } else { 0 };
    // The body as placed, which is what operations take: a placement that scales, shears or
    // mirrors the part maps its geometry without its tolerances, so its check can differ from
    // the part's. A check that cannot run refuses an operation's result too (check_kept). A stop
    // met while deciding the heal is the import's stop, whatever the job asked.
    let check = match check {
        Some(PlacedCheck::Stopped) => Some(PlacedCheck::Stopped),
        made => job.check.then(|| {
            if part.refused.is_some() {
                return PlacedCheck::Skipped;
            }
            made.unwrap_or_else(|| check_placed(&body))
        }),
    };
    Ok(Made { body, missing, part, read, check })
}

fn place(body: &Body, m: &M4) -> std::result::Result<Body, Failure> {
    solidcraft_profile::scope("import", "step.place", || placed(body, m)).map_err(|e| Failure { read: false, msg: e.to_string() })
}

/// SMLib's check of `body` ([`crate::validation::failing_check`]), as [`PlacedCheck`] says it.
fn check_placed(body: &Body) -> PlacedCheck {
    match solidcraft_profile::scope("import", "step.check", || crate::validation::failing_check(body)) {
        Ok(None) => PlacedCheck::Passes,
        Ok(Some(failure)) => PlacedCheck::Fails(failure),
        Err(_) if solidcraft_smlib::stop::current().is_some_and(|s| s.is_stopped()) => PlacedCheck::Stopped,
        Err(e) => PlacedCheck::CouldNotRun(e.to_string()),
    }
}

/// Whether the part just read is healed ([`heal`]), decided by its own check, and this job's
/// body placed with its check when that check is already the one the job needs.
///
/// The heal changes the part every instance shares, so only the part's own check decides it:
/// the part checked where the file defines it, whatever this job's placement, so the instance
/// the import happens to read the part at never changes the decision. A placement maps the
/// geometry and, when it scales, shears or mirrors, not its tolerances, so an instance's
/// verdict is its own: each is placed and checked again by the caller (`None` here), except at
/// the identity, where the part's check is the job's. Only a check that ran and failed is
/// healed: one that passes needs nothing, and one that could not run or was stopped says
/// nothing about the part. A healed part is checked as kept by the heal itself
/// ([`crate::ToleranceHeal::check`]), which is the job's check at the identity.
///
/// A stop is the job's [`PlacedCheck::Stopped`], with the body placed under no token
/// ([`solidcraft_smlib::stop::unstoppable`]): a placement stopped part way would turn the
/// import's stop into a part refused.
pub(super) fn decide_heal(part: &mut Part, read: &mut Vec<Note>, m: &M4, item: u64) -> std::result::Result<Option<(Body, PlacedCheck)>, Failure> {
    let at_identity = *m == IDENTITY;
    let stopped = |part: &Part| -> std::result::Result<Option<(Body, PlacedCheck)>, Failure> {
        Ok(Some((solidcraft_smlib::stop::unstoppable(|| place(&part.body, m))?, PlacedCheck::Stopped)))
    };
    let check = check_placed(&part.body);
    match check {
        PlacedCheck::Fails(_) => {}
        PlacedCheck::Stopped => return stopped(part),
        // The part's own check, deciding nothing: this job's at the identity.
        check => return Ok(at_identity.then(|| (part.body.clone(), check))),
    }
    match solidcraft_profile::scope("import", "step.heal", || heal(part, item)) {
        Healed::Kept(note, kept) => {
            read.push(note);
            Ok(at_identity.then(|| (part.body.clone(), kept.map_or(PlacedCheck::Passes, PlacedCheck::Fails))))
        }
        Healed::Stopped => stopped(part),
        Healed::NotKept => Ok(at_identity.then(|| (part.body.clone(), check))),
        Healed::CouldNotRun(e) => {
            // A panic in the heal is the kernel's internal error: reported, as the import
            // reports a check that cannot run. Any other failure leaves the check's verdict.
            if let KernelError::Internal(why) = &e {
                log::warn!(target: "step", "the heal of item #{item} failed: {e}");
                read.push(Note::HealFailed(format!("the tolerance heal failed: kernel internal error: {why}")));
            } else {
                log::debug!(target: "step", "the heal of item #{item} could not run: {e}");
            }
            Ok(at_identity.then(|| (part.body.clone(), check)))
        }
    }
}

/// What [`heal`] did with a part.
pub(super) enum Healed {
    /// Healed: the part's body and file faces replaced, the note the import shows, and SMLib's
    /// check of the body kept ([`crate::ToleranceHeal::check`]: `None` when it passes).
    Kept(Note, Option<crate::CheckFailure>),
    /// Nothing the heal could prove, or a result whose check is not better: the part as it was.
    NotKept,
    /// The heal could not run (the part as it was): never a reason to change it.
    CouldNotRun(KernelError),
    /// The token over the import's work stopped it (the part as it was): the import is stopped.
    Stopped,
}

/// A part whose check fails, healed of its tolerance-scale defects
/// ([`crate::heal_tolerance_defects`]) when that can be proved and leaves the check failing
/// fewer tests: its body and its faces' file faces (the heal only removes faces) replaced.
/// Otherwise left as it is, and [`Healed`] says why.
pub(super) fn heal(part: &mut Part, item: u64) -> Healed {
    let (body, heal) = match crate::tolerance_heal::heal_tolerance_defects(&part.body) {
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
    part.body = body;
    part.face_origin = face_origin;
    let note = Note::Repaired(format!("healed SMLib's check failures: {}", heal.summary()));
    Healed::Kept(note, heal.check)
}

/// The part's body where its instance goes: the very same body at the identity (it keeps the
/// SMLib body the read made), else its record moved there.
fn placed(body: &Body, m: &M4) -> crate::Result<Body> {
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
        solidcore_step::topology::read_solid_with(ex, item, units, &solidcraft_smlib::pcurves::SmlibTrims)
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
fn solid_part(solid: &solidcore_step::topology::Solid) -> std::result::Result<Part, Failure> {
    let failed = |e: KernelError| Failure { read: false, msg: e.to_string().trim_start_matches("the operation failed: ").to_string() };
    let r = &solid.brep;
    let mut reversed = vec![false; r.faces.len()];
    if solid.sheet {
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
    let (body, of_record) = if reversed.iter().any(|r| *r) {
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
        let natural_origin: Vec<u64> = to_record.iter().map(|f| solid.face_origin.get(*f).copied()).collect::<Option<_>>().unwrap_or_default();
        let (smlib, of_natural) = smlib_body(natural.record()).map_err(|f| Failure { msg: name_record_face(&f.msg, &natural_origin), ..f })?;
        let record_of = |k: usize| of_natural.get(k).and_then(|j| to_record.get(*j)).copied();
        let (back, from) = turn(&smlib, &|k| record_of(k).and_then(|f| reversed.get(f)).copied().unwrap_or(false)).map_err(failed)?;
        let of_record = from.iter().map(|k| record_of(*k)).collect::<Option<Vec<usize>>>().ok_or_else(renumbered)?;
        (back.without_history(), of_record)
    } else {
        smlib_body(r).map_err(|f| Failure { msg: name_record_face(&f.msg, &solid.face_origin), ..f })?
    };
    let face_origin = of_record.iter().map(|f| solid.face_origin.get(*f).copied()).collect::<Option<Vec<u64>>>().ok_or_else(renumbered)?;
    Ok(Part { body, closed: !solid.sheet, file_faces: solid.face_origin.len(), face_origin, refused: None })
}

fn renumbered() -> Failure {
    Failure { read: false, msg: "not supported yet: SMLib renumbered the solid's faces".into() }
}

/// The body SMLib makes of `record` as given, and the record's face each of its faces is.
fn smlib_body(record: &solidcore_record::brep::Brep) -> std::result::Result<(Body, Vec<usize>), Failure> {
    let failed = |e: KernelError| Failure { read: false, msg: e.to_string().trim_start_matches("the operation failed: ").to_string() };
    let (brep, smlib_order) = crate::native::smlib_record_ordered(record).map_err(failed)?;
    let (body, body_order) = crate::native::body_from_smlib_ordered(&brep).map_err(failed)?;
    // SMLib's face i is the record's `smlib_order` face and the body's `body_order` face (an
    // empty order is the same one).
    let Some(body_order) = body_order else { return Err(renumbered()) };
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
    Ok((body, of_record))
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
    let trims = &solidcraft_smlib::pcurves::SmlibTrims;
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
        return Ok(Part { body: body.clone(), closed: false, file_faces, face_origin: vec![*face], refused: Some(refused.to_string()) });
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
    Ok(Part { body, closed, file_faces, face_origin, refused: Some(refused.to_string()) })
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
