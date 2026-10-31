//! Sheet PNG export jobs (#4890 EX2): `drawing.export` with `job` writes a large sheet a band at a
//! time, so the window keeps drawing meanwhile. The job is the session's, as the sheet scenes are
//! ([`crate::drawing_scene::step`]): the UI's tick calls [`step`], a band a call, and
//! `drawing.export.status` reports it in a render job's shape. One job at a time; like a render
//! job, it carries on when another design is opened.

use std::io::Write;

use serde_json::Value;
use solidcraft_io::AtomicFile;
use solidcraft_io::paper::PngWriter;

use crate::Session;
use crate::schema::drawing_export::{ExportJobError, ExportJobStatus, ExportPhase, ExportProgress};

/// The session's export job, and the number the next one takes.
#[derive(Default)]
pub struct Exports {
    serial: u64,
    job: Option<Job>,
}

/// A sheet PNG being written to a file.
struct Job {
    id: u64,
    path: String,
    phase: ExportPhase,
    /// Rows of pixels written, and in all.
    rows: [u32; 2],
    error: Option<String>,
    /// The writer and its file, until the job ends: dropped, the file's temporary file goes.
    writing: Option<(PngWriter, Counted)>,
    /// `drawing.export`'s reply for the file, its size filled in once it is written.
    result: Value,
}

/// The job's file, counting the bytes written for the reply.
struct Counted {
    file: AtomicFile,
    bytes: usize,
}

impl Write for Counted {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(buf)?;
        self.bytes += n;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl Exports {
    /// The job under way, if one is.
    pub(crate) fn running(&self) -> Option<u64> {
        self.job.as_ref().filter(|job| job.phase == ExportPhase::Running).map(|job| job.id)
    }

    /// Start writing `png` to `file` at `path` (checked already), with `drawing.export`'s reply
    /// for it in `result`. It takes the place of a job that has ended.
    pub(crate) fn start(&mut self, path: String, png: PngWriter, file: AtomicFile, result: Value) -> ExportJobStatus {
        self.serial += 1;
        let rows = png.rows();
        let job =
            Job { id: self.serial, path, phase: ExportPhase::Running, rows, error: None, writing: Some((png, Counted { file, bytes: 0 })), result };
        self.job.insert(job).status()
    }

    pub(crate) fn status(&self, id: u64) -> Option<ExportJobStatus> {
        self.job.as_ref().filter(|job| job.id == id).map(Job::status)
    }

    /// Stop job `id` if it is under way: its file isn't written. An ended job stays as it ended.
    pub(crate) fn cancel(&mut self, id: u64) -> Option<ExportJobStatus> {
        let job = self.job.as_mut().filter(|job| job.id == id)?;
        if job.phase == ExportPhase::Running {
            job.writing = None;
            job.phase = ExportPhase::Cancelled;
        }
        Some(job.status())
    }
}

impl Job {
    fn status(&self) -> ExportJobStatus {
        ExportJobStatus {
            job: self.id,
            progress: ExportProgress { phase: self.phase, done: self.rows[0].into(), total: self.rows[1].into() },
            error: self.error.clone().map(|message| ExportJobError { message }),
            result: (self.phase == ExportPhase::Completed).then(|| self.result.clone()),
        }
    }

    /// The next band; after the last, the file ended, its path checked again and the file put in
    /// place. Says whether more bands are waiting: none once the job has ended.
    fn advance(&mut self) -> std::result::Result<bool, String> {
        let Some((png, out)) = &mut self.writing else { return Ok(false) };
        let more = png.step(out).map_err(|e| e.to_string())?;
        self.rows = png.rows();
        if more {
            return Ok(true);
        }
        let Some((png, mut out)) = self.writing.take() else { return Ok(false) };
        png.finish(&mut out).map_err(|e| e.to_string())?;
        solidcraft_io::vfs::check(&self.path).map_err(|e| e.to_string())?;
        let bytes = out.bytes;
        out.file.commit().map_err(|e| e.to_string())?;
        self.result["bytes"] = bytes.into();
        self.phase = ExportPhase::Completed;
        Ok(false)
    }
}

/// Writes the next band of the export job under way, and says whether more are waiting: a short
/// piece of work for the UI's tick. A failure ends the job, its file not written.
pub fn step(s: &mut Session) -> bool {
    let Some(job) = s.drawing_exports.job.as_mut() else { return false };
    job.advance().unwrap_or_else(|why| {
        job.writing = None;
        job.phase = ExportPhase::Failed;
        job.error = Some(why);
        false
    })
}
