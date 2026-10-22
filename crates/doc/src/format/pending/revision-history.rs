//! A design stores `document_history`: the `solidcraft/N` number it was written with
//! (`format_version`), how many times a build that records history has saved it (`revision`,
//! 0 until that first save), and `entries` (oldest first). A `saved` entry can carry a
//! `snapshot`, the parametric document as of that save, with no nested `document_history` and
//! no evaluated boundary representation. The newest 16 snapshots are kept, and their parametric
//! JSON stays within 8 MiB. STEP text, mesh arrays and canvas images count as a digest because
//! a design container stores each distinct payload once by hash. A plain JSON save applies that
//! cap to the encoded bytes and drops the oldest snapshot bodies. An entry without a snapshot
//! still records its revision, format number and action.
//!
//! Older designs have no `document_history`. Opening one records revision 0 and one `opened`
//! entry at the number the file was saved as, with no snapshot, and does not change geometry,
//! parameters or features. Restoring a revision that has no snapshot is refused. A file saved
//! by the current build before this change (the pending format, without this name in
//! `format_pending`) opens the same way. The last release asks for an update instead of
//! dropping the history when it sees this format number.

use serde_json::Value;

/// Give a design that has no readable history the record of the format it was saved as.
pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(obj) = v.as_object_mut() else {
        return Ok(());
    };
    let origin = obj.get("format").and_then(Value::as_str).and_then(crate::format::version_of).unwrap_or(0);
    let history = crate::document_history::DocumentHistory::adopt(obj.get("document_history"), origin);
    let value = serde_json::to_value(&history).map_err(|e| crate::DocError::Invalid(format!("document history: {e}")))?;
    obj.insert("document_history".into(), value);
    Ok(())
}
