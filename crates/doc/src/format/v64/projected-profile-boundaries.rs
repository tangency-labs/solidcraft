//! Whole-body projections saved as traced lines and circular arcs keep their complete
//! single-loop profile identity in the link's `profile_boundary` (the original curve IDs).
//! The step records it before rebuild replaces the trace with exact projected curves. A
//! downstream curve pick then requires the same source's uniquely certified complete loop;
//! partial, split or ambiguous boundaries retain the existing profile error. New projections
//! omit the field and keep their current curve-pick semantics. Older designs without a complete
//! single-loop projection are unchanged. The last release asks for an update instead of meeting
//! a format it cannot read: it warns about the newer format number, opens what it can read and
//! keeps unknown fields untouched when saving (`crates/doc/src/newer_file.rs`).
use serde_json::Value;
use solidcraft_sketch::{CurveKind, LinkKind, LinkSource, Sketch};

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(features) = v.get_mut("features").and_then(Value::as_array_mut) else { return Ok(()) };
    for feature in features {
        if feature.get("type").and_then(Value::as_str) != Some("sketch") {
            continue;
        }
        let Some(raw) = feature.get_mut("sketch") else { continue };
        // Keep migration work bounded even before the typed document has been validated.
        if raw.get("curves").and_then(Value::as_array).is_none_or(|curves| curves.len() > solidcraft_sketch::MAX_LINK_ENTITIES) {
            continue;
        }
        let Ok(sketch) = serde_json::from_value::<Sketch>(raw.clone()) else { continue };
        if sketch.links.len().saturating_mul(sketch.curves.len()) > 4_000_000 {
            continue;
        }
        if !sketch.links.iter().any(|link| {
            !link.lost && link.profile_boundary.is_empty() && link.kind == LinkKind::Project && matches!(link.source, LinkSource::Body { .. })
        }) {
            continue;
        }
        let search = solidcraft_sketch::find_profiles_checked(&sketch);
        if search.incomplete {
            continue;
        }
        let Some(links) = raw.get_mut("links").and_then(Value::as_array_mut) else { continue };
        for (link, written) in sketch.links.iter().zip(links) {
            if link.lost || !link.profile_boundary.is_empty() || link.kind != LinkKind::Project || !matches!(link.source, LinkSource::Body { .. }) {
                continue;
            }
            let output: Vec<_> = sketch.curves.iter().filter(|curve| curve.link.as_deref() == Some(link.id.as_str())).collect();
            if output.is_empty()
                || output.iter().any(|curve| !matches!(curve.kind, CurveKind::Line { .. } | CurveKind::Arc { .. } | CurveKind::Circle { .. }))
            {
                continue;
            }
            let mut ids: Vec<_> = output.iter().map(|curve| curve.id.clone()).collect();
            ids.sort();
            if ids.windows(2).any(|pair| pair[0] == pair[1]) {
                continue;
            }
            let matches = search
                .profiles
                .iter()
                .filter(|profile| profile.whole_curve_boundaries().is_some_and(|(outer, holes)| holes.is_empty() && outer == ids));
            if matches.count() == 1
                && let Some(written) = written.as_object_mut()
            {
                written.insert("profile_boundary".into(), Value::Array(ids.into_iter().map(Value::String).collect()));
            }
        }
    }
    Ok(())
}
