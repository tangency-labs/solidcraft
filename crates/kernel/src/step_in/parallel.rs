//! Bounded desktop preparation of independent imported bodies, collected in file order.
use std::sync::Arc;

use super::{Body, Ctx, Exchange, KernelError, Units, brep, guard, mt};

pub(super) struct Job {
    pub source: Source,
    /// Where the body goes (its instance's placement in the assembly).
    pub placement: mt::Matrix4,
    pub check_mesh: bool,
    /// The file's point-on-surface tolerance (`super::Units::tol`) for the representation this
    /// solid was read from: how far its boundaries may lie off their surfaces (mm). The body
    /// keeps it, so the tessellator holds an imported boundary to the file's own uncertainty
    /// rather than to a fraction of the part's size (see `crate::body::Body::boundary_tol`).
    pub tol: f64,
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
    let (part, read) = match &job.source {
        Source::Oriented(part) => (part.clone(), None),
        Source::Item { item, units } if smlib_backend() => {
            let (part, read) = smlib_part(ex, *item, *units)?;
            (Arc::new(part), Some(read))
        }
        Source::Item { item, units } => {
            let cx = Ctx { ex, len: units.len, ang: units.ang, tol: units.tol, snap: units.snap };
            let out = guard("step brep", || brep::read_item(&cx, *item).map_err(KernelError::Failed))
                .map_err(|e| Failure { read: true, msg: e.to_string().trim_start_matches("the operation failed: ").to_string() })?;
            // Orient once where the file defines the solid; each placement then takes its
            // orientation from the matrix instead of meshing the whole part again.
            let body = guard("STEP body preparation", || Body::new(out.solid)).map_err(|e| Failure { read: false, msg: e.to_string() })?;
            // The file's tolerance is set on the part itself, before anything meshes it: the
            // unmeshed-face count below is the import's own warning and must judge the body by
            // the file's uncertainty too, and every placement of the part carries the figure on.
            let body = body.with_boundary_tol(Some(job.tol));
            (Arc::new(Part { body, closed: out.closed, file_faces: out.file_faces, face_origin: out.face_origin }), Some(out.failed))
        }
    };
    guard("STEP body preparation", || {
        let body = part.body.placed(&job.placement)?;
        // Counted where the orientation mesh already counted them (a placement meshes the
        // same faces), and on the part, which carries the file's tolerance.
        let missing = if job.check_mesh { part.body.unmeshed_faces() } else { 0 };
        Ok((body, missing))
    })
    .map(|(body, missing)| Made { body, missing, part, read })
    .map_err(|e| Failure { read: false, msg: e.to_string() })
}

/// Whether solids are read into SMLib ([`crate::backend`]). A setting this build can't honour
/// reads them on neither kernel: [`smlib_part`] then says why.
fn smlib_backend() -> bool {
    !matches!(crate::backend(), Ok(crate::Backend::Truck))
}

/// A solid item read with the STEP reader into its schema 4 record, then into SMLib (healed
/// where SMLib's check refuses it as given), and brought back as a body: the part SMLib made of
/// the file. A solid the reader or SMLib refuses is not imported, with the reason; there is no
/// other kernel to fall back on.
#[cfg(feature = "smlib")]
pub(super) fn smlib_part(ex: &Exchange, item: u64, units: Units) -> std::result::Result<(Part, Vec<String>), Failure> {
    crate::backend().map_err(|e| Failure { read: true, msg: e.to_string() })?;
    let context =
        solidcraft_step::units::Context { units: solidcraft_step::read::Units { len: units.len, ang: units.ang }, uncertainty: units.uncertainty };
    let solid = solidcraft_step::topology::read_solid(ex, item, context).map_err(|msg| Failure { read: true, msg })?;
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
    let part = Part { body, closed: !solid.sheet, file_faces: solid.face_origin.len(), face_origin };
    Ok((part, solid.notes))
}

#[cfg(not(feature = "smlib"))]
pub(super) fn smlib_part(_: &Exchange, _: u64, _: Units) -> std::result::Result<(Part, Vec<String>), Failure> {
    Err(Failure { read: true, msg: crate::backend().err().map_or_else(|| "this build has no SMLib".into(), |e| e.to_string()) })
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
