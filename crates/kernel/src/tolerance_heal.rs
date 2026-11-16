//! Healing a part's tolerance-scale defects, the ones SMLib's full check fails on an imported
//! part (edges shorter than their tolerance, the sliver faces between two edges that lie
//! within tolerance of each other, curve pieces of no length): solidcore-ops' heal, which
//! decides every edit on a certified bound, grows no tolerance, and keeps its result only
//! when the check of it fails fewer tests and no new kind ([`solidcore_ops::heal`]). Run when
//! a STEP part is read and by Repair.

use crate::body::Body;
use crate::{KernelError, Result};

/// What [`heal_tolerance_defects`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToleranceHeal {
    /// Edges squeezed into one of their vertices.
    pub squeezed_edges: usize,
    /// Edges re-ended at a vertex a squeeze kept.
    pub re_ended_edges: usize,
    /// Sliver faces between two coincident edges removed.
    pub collapsed_faces: usize,
    /// Pieces of no length cut out of edge curves.
    pub cut_pieces: usize,
    /// The largest certified distance any of those edits moved the part's boundary, mm.
    pub largest_bound: f64,
    /// Failed tests of the check before and after.
    pub failed: [usize; 2],
    /// For each face of the healed part, the face of the part it was.
    pub face_source: Vec<usize>,
}

impl ToleranceHeal {
    /// The heal in a line, for an import note or a repair report.
    pub fn summary(&self) -> String {
        let mut done = Vec::new();
        if self.squeezed_edges > 0 {
            done.push(format!("{} edges shorter than their tolerance collapsed into their vertices", self.squeezed_edges));
        }
        if self.collapsed_faces > 0 {
            done.push(format!("{} sliver faces between coincident edges removed", self.collapsed_faces));
        }
        if self.cut_pieces > 0 {
            done.push(format!("{} curve pieces of no length cut out", self.cut_pieces));
        }
        format!(
            "{} (nothing moved more than {:.2e} mm; no tolerance raised); SMLib's check fails {} tests, was {}",
            done.join(", "),
            self.largest_bound,
            self.failed[1],
            self.failed[0]
        )
    }
}

/// `body` healed of its tolerance-scale defects (module docs) and what was done, or `None`
/// when there is nothing the heal can prove or its result would not be better (the body as it
/// is then stands). A mesh body, or one solidcore cannot convert, is `None`.
///
/// # Errors
/// The heal's failures ([`KernelError::Failed`]) and the healed record's.
pub fn heal_tolerance_defects(body: &Body) -> Result<Option<(Body, ToleranceHeal)>> {
    let Some((ops, order)) = body.ops_body() else {
        return Ok(None);
    };
    let (healed, report) = solidcraft_smlib::stop::unstoppable(|| solidcore_ops::heal::heal_tolerance_defects(ops, order))
        .map_err(|e| KernelError::Failed(format!("heal: {e}")))?;
    let Some(healed) = healed else {
        log::debug!(target: "repair", "tolerance heal not kept: {:?}", report.why_not);
        return Ok(None);
    };
    let out = Body::from_record(healed.record)?.with_color(body.color());
    let heal = ToleranceHeal {
        squeezed_edges: report.squeezed_edges,
        re_ended_edges: report.re_ended_edges,
        collapsed_faces: report.collapsed_faces,
        cut_pieces: report.cut_pieces,
        largest_bound: report.largest_bound,
        failed: [report.before.failures, report.after.map_or(0, |a| a.failures)],
        face_source: healed.face_source,
    };
    Ok(Some((out, heal)))
}
