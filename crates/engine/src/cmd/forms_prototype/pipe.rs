//! Pipe source evaluation uses the Form author's frame; stored edit picks are world-space.
use super::*;
use crate::doc::forms::{Definition, Pipe, PipeSource, Placement};
use crate::geom::subdivision::{
    pipe_network::{PipeSettings, SegmentOverride},
    primitives::Parameters,
};
use std::collections::BTreeMap;

fn error(message: impl ToString) -> EngineError {
    EngineError::Other(message.to_string())
}

pub(super) fn state(s: &Session, component: u64, editing: Option<u64>) -> Result<(crate::doc::ModelState, crate::doc::Mat)> {
    let frame = if component == s.active_component { s.active_component_frame() } else { s.doc.component_transform(component) };
    crate::doc::mat_inverse(&frame).ok_or_else(|| error("the pipe author frame is not invertible"))?;
    let before = editing.map_or_else(|| s.model.state(), |id| s.model.state_before(id));
    let state = crate::doc::state_in_frame_at(&s.doc, &before, component, frame).into_owned();
    Ok((state, frame))
}
pub(super) fn recompute(s: &Session, definition: &mut Definition, component: u64, editing: Option<u64>) -> Result<()> {
    let Some(pipe) = &definition.pipe else {
        return Ok(());
    };
    let (state, frame) = state(s, component, editing)?;
    let inverse = crate::doc::mat_inverse(&frame).ok_or_else(|| error("the pipe author frame is not invertible"))?;
    let mut local = pipe.as_ref().clone();
    local.source.transform_picks(&inverse);
    definition.cage = local.recompute(&definition.cage, &state)?;
    Ok(())
}
pub(super) fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::Pipe = parse(p)?;
    if s.active_sketch.is_some() {
        return Err(error("finish the sketch before creating a Form Pipe"));
    }
    if s.forms_prototype.as_ref().is_some_and(|f| f.tool_original.is_some()) {
        return Err(error("apply or cancel the active Form tool first"));
    }
    let current = s.forms_prototype.as_ref();
    if current.is_some_and(|f| !f.parts.is_empty() || f.consumed_features().next().is_some()) {
        return Err(error("finish staged Forms before creating a pipe"));
    }
    let component = current.map_or(s.active_component, |f| f.authoring_component);
    let editing = current.and_then(|f| f.editing_feature);
    let (state, frame) = state(s, component, editing)?;
    let inverse = crate::doc::mat_inverse(&frame).ok_or_else(|| error("the pipe author frame is not invertible"))?;
    let existing = current.and_then(|f| f.pipe.as_deref());
    let mut old_source = existing.map(|p| p.source.clone());
    if let Some(source) = &mut old_source {
        source.transform_picks(&inverse);
    }
    if args.sketch.is_some() && args.body.is_some() || args.sketch.is_some() && args.edges.is_some() || args.body.is_some() && args.curves.is_some() {
        return Err(error("select one sketch path or one body's edges"));
    }
    let sketch = args.sketch.or_else(|| {
        if args.curves.is_some() {
            match &old_source {
                Some(PipeSource::Sketch { sketch, .. }) => Some(sketch.0),
                _ => None,
            }
        } else {
            None
        }
    });
    let body = args.body.clone().or_else(|| {
        if args.edges.is_some() {
            match &old_source {
                Some(PipeSource::Edges { body, .. }) => Some(body.0.clone()),
                _ => None,
            }
        } else {
            None
        }
    });
    let source = if let Some(id) = sketch {
        let solved =
            state.sketches.iter().find(|s| s.feature == id).ok_or_else(|| error("the pipe source sketch is missing or follows the edited Form"))?;
        PipeSource::from_sketch(solved, args.curves.as_deref().unwrap_or_default())?
    } else if let Some(name) = body {
        let body = state.body(&name).ok_or_else(|| error("the pipe source body is missing or follows the edited Form"))?;
        let edges = args.edges.unwrap_or_else(|| (0..body.body.edge_count()).take(129).collect());
        PipeSource::from_edges(body, &edges)?
    } else if args.curves.is_some() || args.edges.is_some() {
        return Err(error("select a pipe source before its curves or edges"));
    } else {
        old_source.clone().ok_or_else(|| error("select a sketch or CAD edge path for the pipe"))?
    };
    let mut settings = existing.map_or_else(PipeSettings::default, |p| p.settings.clone());
    if let Some(d) = args.diameter {
        settings.diameter = d;
    }
    if let Some(n) = args.section_density {
        settings.section_density = n;
    }
    if let Some(n) = args.path_samples {
        settings.path_samples = n;
    }
    if let Some(overrides) = args.overrides {
        if overrides.len() > 128 {
            return Err(error("select at most 128 pipe segment overrides"));
        }
        let mut next = BTreeMap::new();
        for value in overrides {
            if next
                .insert(value.segment, SegmentOverride { diameter: value.diameter.filter(|d| *d != 0.0), samples: value.samples.filter(|n| *n != 0) })
                .is_some()
            {
                return Err(error("pipe segment override identities must be distinct"));
            }
        }
        next.retain(|_, v| v.diameter.is_some() || v.samples.is_some());
        settings.overrides = next;
    }
    settings.validate().map_err(error)?;
    if let Some(existing) = existing
        && !existing.follow_path
        && !args.regenerate
    {
        if old_source.as_ref() == Some(&source) && existing.settings == settings {
            return Ok(current.map_or_else(|| json!({"active":false}), |f| f.inspect()));
        }
        return Err(error("changing a sculpted pipe requires explicit regeneration from its path"));
    }
    let mut pipe = Pipe { source, settings, follow_path: true };
    let built = pipe.rebuild(&state)?;
    let cage = built.cage;
    pipe.source.transform_picks(&frame);
    let placement = if current.is_some_and(|f| f.pipe.is_some()) {
        current.map(|f| f.placement.clone()).ok_or_else(|| error("the pipe edit session ended"))?
    } else {
        Placement::from_matrix(&frame)?
    };
    let diameter = pipe.settings.diameter;
    let definition = Definition {
        imported: None,
        pipe: Some(Box::new(pipe)),
        profile: None,
        matching: None,
        placement,
        cage,
        parameters: Parameters { kind: "pipe".into(), dimensions: [diameter; 3], ..Default::default() },
        hierarchy: None,
        symmetry: None,
    };
    if let Some(current) = &mut s.forms_prototype {
        current.adopt_pipe(definition)?;
        current.pipe_segments = built.segments;
    } else {
        let mut form = crate::forms_prototype::Form::from_definition(definition, 2, None)?;
        form.authoring_component = s.active_component;
        form.pipe_segments = built.segments;
        form.authoring_path = s.active_occurrence_path.clone();
        s.forms_prototype = Some(form);
    }
    Ok(s.forms_prototype.as_ref().map_or_else(|| json!({"active":false}), |f| f.inspect()))
}
