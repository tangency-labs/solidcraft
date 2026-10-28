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
    /// No B-rep could be made of the solid: the body is a mesh of the faces that could be read
    /// on their own ([`mesh_part`]), and this is why the solid was refused.
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
            let (part, read) = smlib_part(ex, *item, *units).or_else(|refused| mesh_part(ex, *item, *units, refused))?;
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
/// refuses fails with the reason ([`mesh_part`] then shows what it can of it).
pub(super) fn smlib_part(ex: &Exchange, item: u64, units: Units) -> std::result::Result<(Part, Vec<String>), Failure> {
    let solid = solidcraft_profile::scope("import", "step.read_solid", || {
        solidcraft_step::topology::read_solid_with(ex, item, units, &solidcraft_smlib::pcurves::SmlibTrims)
    })
    .map_err(|msg| Failure { read: true, msg })?;
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
    let part = Part { body, closed: !solid.sheet, file_faces: solid.face_origin.len(), face_origin, refused: None };
    Ok((part, solid.notes))
}

/// A solid the reader or SMLib refuses, still shown: each of its faces read on its own
/// ([`solidcraft_step::topology::read_faces_with`]) and meshed by the planner as a one-face
/// surface body, the faces' triangles together a mesh body. It is no B-rep (modelling
/// operations refuse it); `refused` says why. Faces that cannot be read or meshed on their own
/// are left out, each named in the notes. With no face to show, the solid stays refused.
fn mesh_part(ex: &Exchange, item: u64, units: Units, refused: Failure) -> std::result::Result<(Part, Vec<String>), Failure> {
    let _span = solidcraft_profile::span("import", "step.refused_mesh");
    let faces = match solidcraft_step::topology::read_faces_with(ex, item, units, &solidcraft_smlib::pcurves::SmlibTrims) {
        Ok(faces) => faces,
        Err(_) => return Err(refused),
    };
    let mut notes = Vec::new();
    let mut bodies = Vec::with_capacity(faces.len());
    for f in &faces {
        match f.record.clone().map_err(KernelError::Failed).and_then(Body::from_record) {
            Ok(b) => bodies.push((f.face, b)),
            Err(e) => notes.push(format!("not shown: {}", e.to_string().trim_start_matches("the operation failed: "))),
        }
    }
    // Every face at the display tolerance of the whole solid.
    let bounds: solidcraft_geom::Aabb3 = bodies.iter().flat_map(|(_, b)| b.record().vertices.iter().map(|v| crate::body::v3(v.point))).collect();
    let size = if bounds.is_empty() { 0.0 } else { bounds.diagonal() };
    let tol = crate::MeshTolerance::display_of(if size.is_finite() { size } else { 0.0 });
    let (mut positions, mut triangles) = (Vec::new(), Vec::new());
    for (face, b) in &bodies {
        match b.mesh(&tol) {
            Ok(m) if !m.triangles.is_empty() => {
                let Ok(base) = u32::try_from(positions.len()) else { break };
                positions.extend_from_slice(&m.positions);
                triangles.extend(m.triangles.iter().map(|t| t.map(|i| i.saturating_add(base))));
            }
            Ok(_) => notes.push(format!("not shown: face #{face}: no triangles")),
            Err(e) => notes.push(format!("not shown: face #{face}: {}", e.to_string().trim_start_matches("the operation failed: "))),
        }
    }
    if triangles.is_empty() {
        return Err(Failure { msg: format!("{} (no face could be shown as a mesh either)", refused.msg), ..refused });
    }
    let body = crate::meshbody::mesh_body(&positions, &triangles)
        .map_err(|e| Failure { msg: format!("{} (its mesh: {e})", refused.msg), read: refused.read })?;
    let closed = body.is_closed_mesh();
    Ok((Part { body, closed, file_faces: faces.len(), face_origin: Vec::new(), refused: Some(refused.msg) }, notes))
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
