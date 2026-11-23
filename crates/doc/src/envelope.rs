//! A component's envelope: the box a Bounding Solid with `contents` set makes around every body
//! placed inside its own component, its subcomponents' at any depth and each of their placements,
//! where they stand in that component's frame. Each body's box is its exact one
//! (`Body::exact_bounds`), taken after the body is placed when the placement turns it, so the
//! envelope is the least axis-aligned box in the component's axes.

use solidcraft_geom::Aabb3;
use solidcraft_kernel as kernel;

use crate::{DocError, Document, FeatureKind, MAX_PLACEMENT_DEPTH, MAX_PLACEMENTS, Mat, ModelBody, ModelState, Result};

/// Most bodies one envelope bounds (placements times bodies).
const MAX_PLACED: usize = 200_000;

/// The bodies placed inside `component` (in `st`) and the transform of each from the frame its
/// geometry is in to `component`'s frame. Bodies another envelope made are left out: an envelope
/// never bounds another.
pub(crate) fn placed_contents<'a>(doc: &Document, st: &'a ModelState, component: u64) -> Result<Vec<(&'a ModelBody, Mat)>> {
    let envelopes: std::collections::HashSet<u64> =
        doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::BoundingSolid { contents: true, .. })).map(|f| f.id).collect();
    let owners = doc.model_body_components(st);
    let mut by_component: std::collections::HashMap<u64, Vec<&ModelBody>> = std::collections::HashMap::new();
    for (b, owner) in st.bodies.iter().zip(&owners) {
        if !envelopes.contains(&b.feature) {
            by_component.entry(*owner).or_default().push(b);
        }
    }
    let mut out = Vec::new();
    // A box around part of the contents could miss a contact: refuse a walk that stopped short.
    let placements = doc.all_placements_within(component).ok_or_else(|| {
        DocError::Invalid(format!(
            "the component places more than {MAX_PLACEMENTS} parts or nests them deeper than {MAX_PLACEMENT_DEPTH} levels, too many to bound"
        ))
    })?;
    for (comp, _, m) in placements {
        for b in by_component.get(&comp).into_iter().flatten() {
            if out.len() >= MAX_PLACED {
                return Err(DocError::Invalid(format!("the component holds more than {MAX_PLACED} placed bodies to bound")));
            }
            let placed = match doc.model_body_offset(b) {
                Some(offset) => crate::mat_mul(&m, offset),
                None => m,
            };
            out.push((*b, placed));
        }
    }
    Ok(out)
}

/// The exact box around `body` placed by `m`.
fn placed_bounds(body: &kernel::Body, m: &Mat) -> Result<Aabb3> {
    let turned = (0..3).any(|c| (0..3).any(|r| (m[c][r] - if c == r { 1.0 } else { 0.0 }).abs() > 1e-12));
    if turned {
        return Ok(kernel::place_matrix(body, *m)?.exact_bounds()?);
    }
    let b = body.exact_bounds()?;
    if b.is_empty() {
        return Ok(b);
    }
    let t = solidcore_record::Vec3::new(m[3][0], m[3][1], m[3][2]);
    Ok(Aabb3 { min: b.min + t, max: b.max + t })
}

/// The exact box around every body placed inside `component`, in its frame (empty when it holds
/// none). The bodies are measured on all cores: an imported assembly places hundreds.
pub(crate) fn contents_bounds(doc: &Document, st: &ModelState, component: u64) -> Result<Aabb3> {
    let placed = placed_contents(doc, st, component)?;
    let boxes = kernel::map_on_threads(&placed, 16, |(b, m)| placed_bounds(&b.body, m));
    let mut bb = Aabb3::EMPTY;
    for (found, (b, _)) in boxes.into_iter().zip(&placed) {
        let found = found.ok_or_else(|| DocError::Invalid(format!("body `{}` could not be measured", b.name)))??;
        bb = bb.union(&found);
    }
    Ok(bb)
}
