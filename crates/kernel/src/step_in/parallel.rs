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
pub(super) struct Part {
    pub body: Body,
    /// The shells are closed.
    pub closed: bool,
    /// Faces the file had.
    pub file_faces: usize,
    /// The file face of each kernel face.
    pub face_origin: Vec<u64>,
    /// No B-rep could be made of the whole solid, and this is why: the body is a surface body of
    /// the faces that read ([`surface_part`]), or, where none could be made of them, a mesh of
    /// them ([`mesh_part`]).
    pub refused: Option<String>,
}

/// A prepared instance.
pub(super) struct Made {
    /// The part, placed.
    pub body: Body,
    /// Faces the mesher missed (counted when the job asked).
    pub missing: usize,
    pub part: Arc<Part>,
    /// This job read the item: the faces the read left out, with the reason.
    pub read: Option<Vec<String>>,
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

fn prepare(ex: &Exchange, job: &Job) -> Prepared {
    let _span = solidcraft_profile::span("import", "step.body");
    let (part, read) = match &job.source {
        Source::Oriented(part) => (part.clone(), None),
        Source::Item { item, units } => {
            let (part, read) = smlib_part(ex, *item, *units).or_else(|refused| surface_part(ex, *item, *units, refused))?;
            (Arc::new(part), Some(read))
        }
    };
    let body = solidcraft_profile::scope("import", "step.place", || placed(&part.body, &job.placement))
        .map_err(|e| Failure { read: false, msg: e.to_string() })?;
    // Counted on the part (a placement meshes the same faces).
    let missing = if job.check_mesh { solidcraft_profile::scope("import", "step.mesh_check", || part.body.unmeshed_faces()) } else { 0 };
    Ok(Made { body, missing, part, read })
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
pub(super) fn smlib_part(ex: &Exchange, item: u64, units: Units) -> std::result::Result<(Part, Vec<String>), Failure> {
    let solid = solidcraft_profile::scope("import", "step.read_solid", || {
        solidcraft_step::topology::read_solid_with(ex, item, units, &solidcraft_smlib::pcurves::SmlibTrims)
    })
    .map_err(|msg| Failure { read: true, msg })?;
    let part = solid_part(&solid)?;
    Ok((part, solid.notes))
}

/// The part SMLib makes of a solid (or sheet) record the reader made.
fn solid_part(solid: &solidcraft_step::topology::Solid) -> std::result::Result<Part, Failure> {
    let failed = |e: KernelError| Failure { read: false, msg: e.to_string().trim_start_matches("the operation failed: ").to_string() };
    let (brep, smlib_order) = crate::native::smlib_record_ordered(&solid.brep).map_err(failed)?;
    let (body, body_order) = crate::native::body_from_smlib_ordered(&brep).map_err(failed)?;
    // The file face of each body face: SMLib's face i is the record's `smlib_order` face and the
    // body's `body_order` face (an empty order is the same one).
    let (Some(smlib_order), Some(body_order)) = (smlib_order, body_order) else {
        return Err(Failure { read: false, msg: "not supported yet: SMLib renumbered the solid's faces".into() });
    };
    let at = |order: &[usize], i: usize| if order.is_empty() { Some(i) } else { order.get(i).copied() };
    let faces = brep.counts().map_err(|e| failed(KernelError::Failed(format!("SMLib: {e}"))))?[0];
    let mut face_origin = vec![0u64; body.face_count()];
    for i in 0..faces {
        let file_face = at(&smlib_order.faces, i).and_then(|r| solid.face_origin.get(r)).copied();
        match (at(&body_order.faces, i).and_then(|b| face_origin.get_mut(b)), file_face) {
            (Some(slot), Some(f)) => *slot = f,
            _ => return Err(Failure { read: false, msg: "not supported yet: SMLib renumbered the solid's faces".into() }),
        }
    }
    Ok(Part { body, closed: !solid.sheet, file_faces: solid.face_origin.len(), face_origin, refused: None })
}

/// Most reads of a refused solid without faces, each leaving out the face the refusal before names.
const MAX_LEFT_OUT_ROUNDS: usize = 8;

/// A solid the reader or SMLib refuses, imported as an open surface body of its exact faces, the
/// faces that make it refused left out and named in the notes; `refused` says why the solid was.
/// Each face is read on its own first ([`solidcraft_step::topology::read_faces_with`]); then:
/// 1. the solid is read again without the faces that cannot be read alone and the face the
///    refusal names, as one sheet whose faces share their edges as the file has them, and made
///    by SMLib; a refusal of that naming another face of the solid leaves it out too (up to
///    [`MAX_LEFT_OUT_ROUNDS`] reads). A face is only ever left out where a refusal names it.
/// 2. Where a refusal names no face of the solid: the faces that read alone, stitched by SMLib
///    into one surface body (their edges sewn at ten times their tolerance).
/// 3. Where SMLib cannot stitch them either: a mesh of them ([`mesh_part`]).
///
/// With no face to import, the solid stays refused.
fn surface_part(ex: &Exchange, item: u64, units: Units, refused: Failure) -> std::result::Result<(Part, Vec<String>), Failure> {
    let _span = solidcraft_profile::span("import", "step.refused_surfaces");
    let trims = &solidcraft_smlib::pcurves::SmlibTrims;
    let Ok(faces) = solidcraft_step::topology::read_faces_with(ex, item, units, trims) else { return Err(refused) };
    let of_solid: std::collections::HashSet<u64> = faces.iter().map(|f| f.face).collect();
    // The faces left out, each with the refusal that names it, in the order they were found.
    let mut left_out: Vec<(u64, String)> = faces.iter().filter_map(|f| f.record.as_ref().err().map(|m| (f.face, m.clone()))).collect();
    let mut why = refused.msg.clone();
    let mut rounds = 0;
    loop {
        // The face the last refusal names, if it is one of the solid's still in, is left out too.
        let named = solidcraft_step::topology::named_face(&why).filter(|f| of_solid.contains(f) && !left_out.iter().any(|(g, _)| g == f));
        match named {
            Some(f) => left_out.push((f, why.clone())),
            None if rounds > 0 => break,
            None => {}
        }
        if left_out.is_empty() || left_out.len() >= faces.len() || rounds >= MAX_LEFT_OUT_ROUNDS {
            break;
        }
        rounds += 1;
        let skip: std::collections::HashSet<u64> = left_out.iter().map(|(f, _)| *f).collect();
        let read =
            solidcraft_profile::scope("import", "step.read_without", || solidcraft_step::topology::read_solid_without(ex, item, units, trims, &skip));
        match read.map_err(|msg| Failure { read: true, msg }).and_then(|solid| Ok((solid_part(&solid)?, solid.notes))) {
            Ok((mut part, mut notes)) => {
                part.file_faces = faces.len();
                part.refused = Some(refused.msg);
                notes.extend(left_out.iter().map(|(_, m)| format!("left out: {m}")));
                return Ok((part, notes));
            }
            Err(e) => {
                log::debug!(target: "step", "item #{item}: read without {} faces, refused: {}", skip.len(), e.msg);
                why = e.msg;
            }
        }
    }
    // The faces that read alone, each a one-face surface body.
    let mut notes = Vec::new();
    let mut pieces = Vec::with_capacity(faces.len());
    for f in &faces {
        match f.record.clone().map_err(KernelError::Failed).and_then(Body::from_record) {
            Ok(b) => pieces.push((f.face, b)),
            Err(e) => notes.push(format!("left out: {}", e.to_string().trim_start_matches("the operation failed: "))),
        }
    }
    match stitched_part(&pieces, faces.len(), &refused.msg) {
        Ok(part) => Ok((part, notes)),
        Err(e) => {
            log::debug!(target: "step", "item #{item}: its faces not stitched ({e}); meshed instead");
            mesh_part(&pieces, faces.len(), notes, refused)
        }
    }
}

/// The one-face surface bodies `pieces` (each with its file face) stitched by SMLib into one
/// surface body, where their edges coincide within ten times the pieces' tolerance.
fn stitched_part(pieces: &[(u64, Body)], file_faces: usize, refused: &str) -> crate::Result<Part> {
    if pieces.is_empty() {
        return Err(KernelError::Failed("no face read".into()));
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

/// The one-face surface bodies `pieces` meshed by the planner, their triangles together a mesh
/// body: no B-rep (modelling operations refuse it), for a solid whose faces SMLib could not
/// stitch either. Faces that cannot be meshed are left out, each named in the notes; with no
/// face to show, the solid stays refused.
fn mesh_part(
    pieces: &[(u64, Body)],
    file_faces: usize,
    mut notes: Vec<String>,
    refused: Failure,
) -> std::result::Result<(Part, Vec<String>), Failure> {
    let _span = solidcraft_profile::span("import", "step.refused_mesh");
    // Every face at the display tolerance of the whole solid.
    let bounds: solidcraft_geom::Aabb3 = pieces.iter().flat_map(|(_, b)| b.record().vertices.iter().map(|v| crate::body::v3(v.point))).collect();
    let size = if bounds.is_empty() { 0.0 } else { bounds.diagonal() };
    let tol = crate::MeshTolerance::display_of(if size.is_finite() { size } else { 0.0 });
    let (mut positions, mut triangles) = (Vec::new(), Vec::new());
    for (face, b) in pieces {
        match b.mesh(&tol) {
            Ok(m) if !m.triangles.is_empty() => {
                let Ok(base) = u32::try_from(positions.len()) else { break };
                positions.extend_from_slice(&m.positions);
                triangles.extend(m.triangles.iter().map(|t| t.map(|i| i.saturating_add(base))));
            }
            Ok(_) => notes.push(format!("left out: face #{face}: no triangles")),
            Err(e) => notes.push(format!("left out: face #{face}: {}", e.to_string().trim_start_matches("the operation failed: "))),
        }
    }
    if triangles.is_empty() {
        return Err(Failure { msg: format!("{} (no face could be shown as a mesh either)", refused.msg), ..refused });
    }
    let body = crate::meshbody::mesh_body(&positions, &triangles)
        .map_err(|e| Failure { msg: format!("{} (its mesh: {e})", refused.msg), read: refused.read })?;
    let closed = body.is_closed_mesh();
    Ok((Part { body, closed, file_faces, face_origin: Vec::new(), refused: Some(refused.msg) }, notes))
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
