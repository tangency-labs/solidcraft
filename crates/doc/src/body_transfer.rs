//! Recorded ownership changes are evaluated at their timeline position.
use crate::{DocError, Document, Mat, ModelBody, ModelState, Result};
use std::collections::BTreeSet;

pub fn valid_transfer_matrix(m: &Mat) -> bool {
    crate::is_placement(m)
}

/// Validate the stored boundary before allocation, loading, or evaluation.
pub fn validate_body_transfer(doc: &Document, bodies: &[String], from: u64, to: u64, transform: &Mat) -> Result<()> {
    if bodies.is_empty()
        || bodies.len() > 1000
        || bodies.iter().any(|name| name.trim().is_empty() || name.len() > 512 || name.chars().any(char::is_control))
        || bodies.iter().collect::<BTreeSet<_>>().len() != bodies.len()
        || !valid_transfer_matrix(transform)
        || [from, to].iter().any(|id| *id != 0 && !doc.components.iter().any(|c| c.id == *id))
    {
        return Err(DocError::Invalid("invalid body transfer boundary".into()));
    }
    Ok(())
}

/// Moves the bodies in `st`, which the evaluator discards when this fails (so it is changed in
/// place rather than copied: a copy of every body for each moved one was most of the cost).
pub(crate) fn evaluate(doc: &Document, st: &mut ModelState, bodies: &[String], from: u64, to: u64, transform: Mat) -> Result<()> {
    validate_body_transfer(doc, bodies, from, to, &transform)?;
    let mut moved = Vec::with_capacity(bodies.len());
    for name in bodies {
        let (slot, old) =
            st.bodies.iter().enumerate().find(|(_, b)| b.name == *name).ok_or_else(|| DocError::Unknown(format!("transferred body `{name}`")))?;
        if doc.model_body_component(old) != from {
            return Err(DocError::Invalid(format!("body `{name}` no longer belongs to transfer source component {from}")));
        }
        let shape = if crate::is_identity(&transform) { old.body.clone() } else { solidcraft_kernel::transform_matrix(&old.body, transform)? };
        let mut body = ModelBody::new(old.name.clone(), shape, old.feature);
        body.component = to;
        body.transferred = true;
        body.source_name = old.source_name.clone();
        body.source_transform = crate::mat_mul(&transform, &old.source_transform);
        body.imported = old.imported.clone();
        body.names = crate::naming::NameCell::same(old);
        moved.push((slot, body));
    }
    for (slot, body) in moved {
        if let Some(b) = st.bodies.get_mut(slot) {
            *b = body;
        }
    }
    crate::body_transfer_sheet::remap_sheet_metadata(st, bodies, &transform)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_transfer_constructor_preserves_document_and_allocator() {
        let mut doc = Document::default();
        let before = doc.to_json();
        let mut scaled = crate::IDENTITY;
        scaled[0][0] = 2.0;
        for (bodies, from, transform) in [
            (vec!["Block".into()], 42, crate::IDENTITY),
            (vec!["Block".into(), "Block".into()], 0, crate::IDENTITY),
            (vec![String::new()], 0, crate::IDENTITY),
            (vec!["   ".into()], 0, crate::IDENTITY),
            (vec!["bad\nname".into()], 0, crate::IDENTITY),
            (vec!["Block".into()], 0, scaled),
        ] {
            assert!(
                doc.add_feature(
                    crate::FeatureKind::BodyTransfer { bodies: bodies.into_iter().map(crate::BodyName).collect(), from, to: 0, transform },
                    None
                )
                .is_err()
            );
            assert_eq!(doc.to_json(), before);
        }
    }
}
