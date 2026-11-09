//! World-frame metadata at a timeline body ownership boundary.
use solidcore_record::Plane;
use solidcraft_geom::prelude::*;

use crate::{DocError, Mat, ModelState, Result, apply_point, apply_vector};

pub(crate) fn remap_sheet_metadata(st: &mut ModelState, names: &[String], transform: &Mat) -> Result<()> {
    let sheets = st.sheets.len().saturating_add(st.surface_sheets.len());
    if names.len() > 1000 || sheets.saturating_add(st.threads.len()) > 100_000 || !transform.iter().flatten().all(|v| v.is_finite()) {
        return Err(DocError::Invalid("body transfer metadata exceeds its finite processing budget".into()));
    }
    let names: std::collections::BTreeSet<&str> = names.iter().map(String::as_str).collect();
    if !st.sheets.iter().any(|s| names.contains(s.body.as_str()))
        && !st.surface_sheets.iter().any(|s| names.contains(s.body.as_str()))
        && !st.threads.iter().any(|t| names.contains(t.body.as_str()))
    {
        return Ok(());
    }
    if !crate::valid_transfer_matrix(transform) {
        return Err(DocError::Invalid("sheet and thread transfer requires a rigid placement".into()));
    }
    let frames: Vec<_> = st
        .sheets
        .iter()
        .enumerate()
        .filter(|(_, s)| names.contains(s.body.as_str()))
        .map(|(i, s)| {
            let frame = Plane::new(apply_point(transform, s.frame.origin), apply_vector(transform, s.frame.x), apply_vector(transform, s.frame.y))
                .ok_or_else(|| DocError::Invalid("invalid transferred sheet frame".into()))?;
            Ok((i, frame))
        })
        .collect::<Result<_>>()?;
    let threads: Vec<_> = st
        .threads
        .iter()
        .enumerate()
        .filter(|(_, t)| names.contains(t.body.as_str()))
        .map(|(i, t)| {
            let point = apply_point(transform, t.axis_point);
            let axis = apply_vector(transform, *t.axis).unit().ok_or_else(|| DocError::Invalid("invalid transferred thread axis".into()))?;
            if !point.is_finite() {
                return Err(DocError::Invalid("invalid transferred thread placement".into()));
            }
            Ok((i, point, axis))
        })
        .collect::<Result<_>>()?;
    for (i, frame) in frames {
        if let Some(s) = st.sheets.get_mut(i) {
            s.frame = frame;
        }
    }
    // A sheet made from surfaces moves whole: its panels' places and its flat (see
    // `SurfaceSheet::flat`), so every piece of it lies where it did on the moved body.
    let mut moved = Vec::new();
    for sk in st.surface_sheets.iter().filter(|s| names.contains(s.body.as_str())) {
        moved.push(sk.copied(&sk.body, transform).ok_or_else(|| DocError::Invalid("invalid transferred sheet placement".into()))?);
    }
    for sk in moved {
        if let Some(slot) = st.surface_sheets.iter_mut().find(|s| s.body == sk.body) {
            *slot = sk;
        }
    }
    for (i, point, axis) in threads {
        if let Some(t) = st.threads.get_mut(i) {
            t.axis_point = point;
            t.axis = axis;
        }
    }
    Ok(())
}
