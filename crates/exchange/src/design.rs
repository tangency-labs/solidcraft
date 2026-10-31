//! An opened design: its parameters, and snapshots of it with overrides and edits applied to a
//! copy.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use solidcraft_doc::{Document, Model, ModelBody, ModelState};

use crate::convert::{self, Budget};
use crate::error::{Error, FeatureFailure, Result};
use crate::options::{Options, Override, Tolerance};
use crate::schema::{
    AppliedEdit, BodyName, Edit, Failure, FeatureId, LengthUnit, Parameter, ParameterName, ParameterValue, Plane, Quantity, SCHEMA, Snapshot,
};
use crate::{
    MAX_BODIES, MAX_CONTROL_POINTS, MAX_EDITS, MAX_EXPRESSION_BYTES, MAX_FACES, MAX_NAME_BYTES, MAX_OVERRIDES, MAX_TOLERANCE_MM, MAX_TRIANGLES,
    MAX_VALUE, MIN_TOLERANCE_MM, RELATIVE_TOLERANCE,
};

/// An opened design. Snapshots of it share one evaluation, so a second snapshot recomputes only
/// the features its overrides and edits change.
#[derive(Debug)]
pub struct Design {
    pub(crate) doc: Document,
    pub(crate) model: Mutex<Model>,
}

impl Design {
    /// The design's name.
    pub fn name(&self) -> &str {
        &self.doc.name
    }

    /// Every parameter (user parameters, sketch dimensions, named feature inputs, text
    /// parameters) at the design's own values.
    pub fn parameters(&self) -> Vec<Parameter> {
        convert::parameters(&self.doc)
    }

    /// The design with the options' overrides and edits applied, evaluated and packaged as a
    /// [`Snapshot`]. The design itself is left as it is. Fails, naming them, when timeline
    /// features fail.
    pub fn snapshot(&self, options: &Options) -> Result<Snapshot> {
        let tolerance = checked_tolerance(options.tolerance)?;
        let allowed = options.failures_allowed;
        let mut doc = self.with_overrides(&options.overrides)?;
        let edits = self.apply_edits(&mut doc, &options.edits, allowed)?;
        let (state, failed) = self.evaluate_allowing(&doc, allowed)?;
        let world = solidcraft_doc::world_state(&doc, &state);
        if let Some(wanted) = &options.bodies {
            if wanted.len() > MAX_BODIES {
                return Err(Error::Limit(format!("more than {MAX_BODIES} bodies asked for")));
            }
            if let Some(missing) = wanted.iter().find(|w| !world.bodies.iter().any(|b| b.name == w.as_str())) {
                return Err(Error::NoBody(missing.clone()));
            }
        }
        let chosen: Vec<&ModelBody> =
            world.bodies.iter().filter(|b| options.bodies.as_ref().is_none_or(|w| w.iter().any(|n| n.as_str() == b.name))).collect();
        if chosen.len() > MAX_BODIES {
            return Err(Error::Limit(format!("{} bodies, over {MAX_BODIES}", chosen.len())));
        }
        let mut budget = Budget { faces: MAX_FACES, triangles: MAX_TRIANGLES, control_points: MAX_CONTROL_POINTS };
        let bodies = chosen.into_iter().map(|b| convert::body(&doc, b, tolerance, options.mass, &mut budget)).collect::<Result<Vec<_>>>()?;
        let used: BTreeSet<&str> = bodies.iter().filter_map(|b| b.material.as_ref().map(|m| m.as_str())).collect();
        let materials = used.into_iter().map(|m| convert::material(&doc, m)).collect();
        let features = convert::features(&doc, &state, &bodies);
        Ok(Snapshot {
            schema: SCHEMA,
            unit: LengthUnit::Mm,
            design: doc.name.clone(),
            parameters: convert::parameters(&doc),
            materials,
            bodies,
            features,
            edits,
            failures: failed.into_iter().map(|f| Failure { feature: f.id, name: f.name, message: f.message }).collect(),
        })
    }

    /// The model at `doc` and the features that failed; refused when one does, unless `allowed`.
    fn evaluate_allowing(&self, doc: &Document, allowed: bool) -> Result<(Arc<ModelState>, Vec<FeatureFailure>)> {
        let mut model = self.model.lock().unwrap_or_else(|e| e.into_inner());
        model.evaluate(doc);
        let failed: Vec<FeatureFailure> = model
            .results
            .iter()
            .filter_map(|r| r.error.as_ref().map(|e| FeatureFailure { id: FeatureId(r.id), name: r.name.clone(), message: e.clone() }))
            .collect();
        if !failed.is_empty() && !allowed {
            return Err(Error::Evaluation(failed));
        }
        Ok((model.state(), failed))
    }

    /// A copy of the design with the overrides applied, all or none.
    fn with_overrides(&self, overrides: &[(ParameterName, Override)]) -> Result<Document> {
        if overrides.len() > MAX_OVERRIDES {
            return Err(Error::Limit(format!("{} parameter overrides, over {MAX_OVERRIDES}", overrides.len())));
        }
        let mut doc = self.doc.clone();
        if overrides.is_empty() {
            return Ok(doc);
        }
        let params = convert::parameters(&doc);
        let mut changes = Vec::with_capacity(overrides.len());
        for (name, value) in overrides {
            let p = params.iter().find(|p| p.name == *name).ok_or_else(|| Error::UnknownParameter(name.clone()))?;
            let bad = |reason: String| Error::ParameterValue { name: name.clone(), reason };
            let expression = match value {
                Override::Value(q) => {
                    let (v, unit) = match (q, &p.value) {
                        (Quantity::Length(l), ParameterValue::Quantity(Quantity::Length(_))) => (l.0, " mm"),
                        (Quantity::Angle(a), ParameterValue::Quantity(Quantity::Angle(_))) => (a.0, " rad"),
                        (Quantity::Number(n), ParameterValue::Quantity(Quantity::Number(_))) => (*n, ""),
                        (q, ParameterValue::Text(_)) => return Err(bad(format!("a text parameter takes a quoted text, not {q:?}"))),
                        (q, _) => return Err(bad(format!("{q:?} is not of the parameter's kind ({:?})", p.value))),
                    };
                    if !v.is_finite() || v.abs() > MAX_VALUE {
                        return Err(bad(format!("{v} is not a finite number of at most {MAX_VALUE:e}")));
                    }
                    // `{}` writes the shortest text that reads back as the same number.
                    format!("{v}{unit}")
                }
                Override::Expression(e) => {
                    if e.len() > MAX_EXPRESSION_BYTES || e.trim().is_empty() || e.chars().any(char::is_control) {
                        return Err(bad(format!("the expression is empty, has control characters or is over {MAX_EXPRESSION_BYTES} bytes")));
                    }
                    e.clone()
                }
            };
            changes.push((name.0.clone(), expression));
        }
        doc.change_params_atomic(&changes).map_err(|e| Error::Overrides(e.to_string()))?;
        Ok(doc)
    }

    /// `edits` made to `doc`, in order: suppressions set on their features, partitions and face
    /// splits added as features at the end of the timeline, each from the model the edits
    /// before it left.
    fn apply_edits(&self, doc: &mut Document, edits: &[Edit], allowed: bool) -> Result<Vec<AppliedEdit>> {
        if edits.len() > MAX_EDITS {
            return Err(Error::Limit(format!("{} edits, over {MAX_EDITS}", edits.len())));
        }
        let mut applied = Vec::with_capacity(edits.len());
        for edit in edits {
            let (features, bodies) = match edit {
                Edit::Suppress(id) => {
                    doc.feature_mut(id.0).ok_or(Error::NoFeature(*id))?.suppressed = true;
                    (vec![*id], Vec::new())
                }
                Edit::Partition { name, plane, bodies } => {
                    let cut_plane = edit_plane("partition", name, plane)?;
                    let (state, _) = self.evaluate_allowing(doc, allowed)?;
                    if let Some(missing) = bodies.iter().find(|b| state.body(b.as_str()).is_none()) {
                        return Err(Error::NoBody(missing.clone()));
                    }
                    let crossed: Vec<BodyName> = state
                        .bodies
                        .iter()
                        .filter(|b| bodies.is_empty() || bodies.iter().any(|n| n.as_str() == b.name))
                        .filter(|b| crosses(b, plane))
                        .map(|b| BodyName(b.name.clone()))
                        .collect();
                    let mut features = Vec::with_capacity(crossed.len());
                    for body in &crossed {
                        let kind = solidcraft_doc::FeatureKind::Split {
                            body: solidcraft_doc::BodyName(body.0.clone()),
                            plane: solidcraft_doc::PlaneRef::Custom { plane: cut_plane },
                            tool: None,
                            sketch: None,
                            line: None,
                        };
                        let label = if crossed.len() > 1 { format!("{name} ({body})") } else { name.clone() };
                        features
                            .push(FeatureId(doc.add_feature(kind, Some(&label)).map_err(|e| Error::Invalid(format!("partition `{name}`: {e}")))?));
                    }
                    (features, crossed)
                }
                Edit::SplitFace { name, body, face, plane } => {
                    let cut_plane = edit_plane("split_face", name, plane)?;
                    let (state, _) = self.evaluate_allowing(doc, allowed)?;
                    if state.body(body.as_str()).is_none() {
                        return Err(Error::NoBody(body.clone()));
                    }
                    let at = solidcraft_doc::naming::named_face_point(&state, Some(body.as_str()), face.as_str())
                        .map_err(|_| Error::NoFace { body: body.clone(), face: face.clone() })?;
                    let kind = solidcraft_doc::FeatureKind::SplitFace {
                        faces: vec![solidcraft_doc::FacePick(at)],
                        body: Some(solidcraft_doc::BodyName(body.0.clone())),
                        plane: solidcraft_doc::PlaneRef::Custom { plane: cut_plane },
                        tool: None,
                        sketch: None,
                    };
                    let id = doc.add_feature(kind, Some(name)).map_err(|e| Error::Invalid(format!("split_face `{name}`: {e}")))?;
                    (vec![FeatureId(id)], vec![body.clone()])
                }
            };
            applied.push(AppliedEdit { edit: edit.clone(), features, bodies });
        }
        Ok(applied)
    }
}

/// Whether `plane` crosses `b`: its mesh has points on both sides.
fn crosses(b: &ModelBody, plane: &Plane) -> bool {
    let eps = (b.body.size() * 1e-6).max(1e-9);
    let [nx, ny, nz] = plane.normal.get();
    let [px, py, pz] = plane.point.0;
    let side = |p: &solidcraft_geom::Vec3| (p.x - px) * nx + (p.y - py) * ny + (p.z - pz) * nz;
    let m = b.mesh();
    m.positions.iter().any(|p| side(p) > eps) && m.positions.iter().any(|p| side(p) < -eps)
}

/// The kernel's plane for an edit's, refused when its point is not finite or the name is empty
/// or too long.
fn edit_plane(kind: &str, name: &str, plane: &Plane) -> Result<solidcraft_geom::Plane> {
    if name.trim().is_empty() || name.len() > MAX_NAME_BYTES || name.chars().any(char::is_control) {
        return Err(Error::Invalid(format!("{kind}: a name of 1 to {MAX_NAME_BYTES} bytes without control characters is needed")));
    }
    let [x, y, z] = plane.point.0;
    let [nx, ny, nz] = plane.normal.get();
    if [x, y, z].iter().any(|c| !c.is_finite() || c.abs() > MAX_VALUE) {
        return Err(Error::Invalid(format!("{kind} `{name}`: the plane's point must be finite")));
    }
    solidcraft_geom::Plane::from_normal(solidcraft_geom::Vec3::new(x, y, z), solidcraft_geom::Vec3::new(nx, ny, nz))
        .ok_or_else(|| Error::Invalid(format!("{kind} `{name}`: the plane's normal has no direction")))
}

/// The chord tolerance, refused when out of range.
fn checked_tolerance(t: Tolerance) -> Result<Tolerance> {
    let ok = match t {
        Tolerance::Relative(r) => r.is_finite() && (RELATIVE_TOLERANCE.0..=RELATIVE_TOLERANCE.1).contains(&r),
        Tolerance::Absolute(a) => a.is_finite() && (MIN_TOLERANCE_MM..=MAX_TOLERANCE_MM).contains(&a),
    };
    if ok {
        Ok(t)
    } else {
        Err(Error::Invalid(format!(
            "chord tolerance {t:?} is out of range (relative {:e} to {}, absolute {MIN_TOLERANCE_MM} to {MAX_TOLERANCE_MM} mm)",
            RELATIVE_TOLERANCE.0, RELATIVE_TOLERANCE.1
        )))
    }
}
