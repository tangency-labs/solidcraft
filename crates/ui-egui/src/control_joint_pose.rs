//! The solved joint pose the preview shows, for a client that follows a joint (#19437).
//!
//! The pose is the one the viewport shows: the joints a preview posed, with the values the
//! solver reached (after limits, motion links, contact and closed loops). It is read, never
//! computed: the numbers come from the preview that is up, not from a new solve.

use serde_json::{Value, json};

use crate::SolveApp;

/// `control.joint_pose`: the pose the preview shows, and the design it belongs to.
///
/// The reply holds the design's id and its public revision (`document.revision`), whether a
/// newer preview is computing, the current error, and the completed pose: what posed it, the
/// generation that counts the completed poses, and the joints it posed with their effective
/// values. The pose is `null` when no solved joint preview is up.
pub(crate) fn snapshot(app: &mut SolveApp) -> Value {
    let revision = app.session.design_revision();
    let document_id = app.session.document_id.clone();
    let (busy, error, pose) = if crate::joint_drag::active(app) {
        let (pose, error) = crate::joint_drag::pose_snapshot(app, revision).unwrap_or((None, None));
        (false, error.map(str::to_owned), pose)
    } else {
        let busy = app.preview.busy;
        let error = app.preview.error.clone();
        (busy, error, app.preview.pose(revision))
    };
    let pose = pose.map(|pose| {
        json!({
            "source": pose.source,
            "generation": pose.generation,
            "joints": pose.joints,
        })
    });
    json!({
        "document_id": document_id,
        "revision": revision,
        "busy": busy,
        "error": error,
        "pose": pose,
    })
}
