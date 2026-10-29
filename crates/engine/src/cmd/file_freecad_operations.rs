//! Rebuild supported FreeCAD history in dependency order, with explicit body targets.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::{Value, json};
use solidcraft_doc::{
    AxisRef, BodyName, Dir, Direction, Extent, ExtrudeConvention, FeatureKind, Operation, PatternKind, PlaneRef, Position, RevolveConvention,
    SketchId,
};
use solidcraft_geom::{Vec2, Vec3};
use solidcraft_io::{FreeCadAxis, FreeCadOperation, FreeCadOperationSettings, FreeCadWarning, FreeCadWarningSeverity};

use crate::{Result, Session, params::bad};

pub(super) struct ImportedSketch {
    pub id: u64,
    pub label: String,
    pub component: u64,
    pub body: Option<(String, String)>,
    pub curves: Vec<Option<String>>,
    pub needs_repair: bool,
    pub warnings: Vec<FreeCadWarning>,
}

pub(super) fn plan(operations: &[FreeCadOperation], params: &Value) -> Result<Vec<FreeCadOperation>> {
    let cmd = "file.import_freecad_sketches";
    let by_name: BTreeMap<&str, &FreeCadOperation> = operations.iter().map(|op| (op.source_name.as_str(), op)).collect();
    let mut pending: Vec<String> = match params.get("operations") {
        None => operations.iter().map(|op| op.source_name.clone()).collect(),
        Some(Value::Array(values)) if values.len() <= 50_000 => values
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or_else(|| bad(cmd, "`operations` must list source operation names")))
            .collect::<Result<_>>()?,
        Some(_) => return Err(bad(cmd, "`operations` must list source operation names")),
    };
    let mut selected = BTreeSet::new();
    while let Some(name) = pending.pop() {
        let operation = by_name.get(name.as_str()).ok_or_else(|| bad(cmd, format!("unknown FreeCAD operation `{name}`")))?;
        if selected.insert(name)
            && let Some(base) = &operation.base
            && by_name.contains_key(base.as_str())
        {
            pending.push(base.clone());
        }
    }
    let mut indegrees = BTreeMap::new();
    let mut children = BTreeMap::<&str, Vec<&str>>::new();
    for name in &selected {
        let op = by_name.get(name.as_str()).ok_or_else(|| bad(cmd, "invalid operation dependency"))?;
        let degree = usize::from(op.base.as_ref().is_some_and(|base| selected.contains(base)));
        indegrees.insert(name.as_str(), degree);
        if let Some(base) = op.base.as_deref().filter(|base| selected.contains(*base)) {
            children.entry(base).or_default().push(name);
        }
    }
    let mut ready: VecDeque<&str> = indegrees.iter().filter_map(|(&name, &count)| (count == 0).then_some(name)).collect();
    let mut ordered = Vec::new();
    while let Some(name) = ready.pop_front() {
        if let Some(op) = by_name.get(name) {
            ordered.push((*op).clone());
        }
        for child in children.get(name).into_iter().flatten() {
            if let Some(count) = indegrees.get_mut(child) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    ready.push_back(child);
                }
            }
        }
    }
    if ordered.len() != selected.len() {
        return Err(bad(cmd, "the FreeCAD operation history contains a dependency cycle"));
    }
    Ok(ordered)
}

fn warning(summary: &str, detail: impl Into<String>) -> FreeCadWarning {
    FreeCadWarning { summary: summary.into(), detail: detail.into(), severity: FreeCadWarningSeverity::Issue, context: None }
}

fn advisory(summary: &str, detail: impl Into<String>) -> FreeCadWarning {
    FreeCadWarning { summary: summary.into(), detail: detail.into(), severity: FreeCadWarningSeverity::Warning, context: None }
}

fn axis(s: &Session, sketch: &ImportedSketch, source: &FreeCadAxis) -> std::result::Result<(AxisRef, Vec3, Vec3), String> {
    let state = s.model.state();
    let solved = state.sketch(sketch.id).ok_or("The profile sketch could not be evaluated.")?;
    let plane = solved.plane;
    Ok(match source {
        FreeCadAxis::SketchX => (AxisRef::SketchAxis { axis: "x".into() }, plane.origin, plane.x),
        FreeCadAxis::SketchY => (AxisRef::SketchAxis { axis: "y".into() }, plane.origin, plane.y),
        FreeCadAxis::SketchLine { geometry } => {
            let curve = sketch.curves.get(*geometry).and_then(Option::as_ref).ok_or("The source axis geometry was not imported.")?;
            let index = solved.sketch.curve_index(curve).ok_or("The axis curve no longer exists.")?;
            let Some(solidcraft_sketch::CurveKind::Line { a, b }) = solved.sketch.curves.get(index).map(|curve| &curve.kind) else {
                return Err("The axis geometry is not a line.".into());
            };
            let (a, b) = solved.sketch.point(*a).zip(solved.sketch.point(*b)).ok_or("The axis endpoints are unavailable.")?;
            (AxisRef::SketchLine { curve: curve.clone() }, plane.to_world(a), plane.dir_to_world(b - a))
        }
        FreeCadAxis::Line { origin, dir } => (AxisRef::Line { origin: (*origin).into(), dir: (*dir).into() }, *origin, *dir),
    })
}

fn kind(
    s: &Session,
    sketch: &ImportedSketch,
    settings: &FreeCadOperationSettings,
    targets: &[String],
    has_base: bool,
) -> std::result::Result<FeatureKind, String> {
    let operation = |cut| {
        if cut {
            Operation::Cut
        } else if has_base {
            Operation::Join
        } else {
            Operation::NewBody
        }
    };
    let profiles = super::super::features::default_extrude_profiles(s, sketch.id);
    let mut feature = match settings {
        FreeCadOperationSettings::Extrude { length, length2, symmetric, reversed, cut, through_all, taper } => FeatureKind::Extrude {
            sketch: SketchId(sketch.id),
            profiles,
            extent: Extent {
                distance: format!("{length} mm").into(),
                convention: ExtrudeConvention::TotalDistance,
                direction: if *symmetric {
                    Direction::Symmetric
                } else if *cut != *reversed {
                    Direction::Negative
                } else {
                    Direction::Positive
                },
                distance2: length2.map(|v| format!("{v} mm").into()),
                start_offset: None,
                through_all: *through_all,
                taper: (taper.abs() > 1e-9).then(|| format!("{taper} deg").into()),
                to: None,
                to_offset: None,
                from: None,
            },
            operation: operation(*cut),
            targets: targets.iter().cloned().map(BodyName).collect(),
            thin: None,
        },
        FreeCadOperationSettings::Revolve { angle, angle2, symmetric, reversed, cut, axis: source_axis } => {
            let (axis, origin, direction) = axis(s, sketch, source_axis)?;
            let state = s.model.state();
            let solved = state.sketch(sketch.id).ok_or("The revolution's sketch could not be evaluated.")?;
            let unit = direction.normalized().ok_or("The revolution axis has no direction.")?;
            if unit.dot(solved.plane.normal()).abs() > 1e-7 || solved.plane.height(origin).abs() > 1e-6 {
                return Err("The revolution axis does not lie in the sketch plane.".into());
            }
            let local_origin = solved.plane.to_local(origin);
            let local_dir = Vec2::new(unit.dot(solved.plane.x), unit.dot(solved.plane.y));
            let mut signs = BTreeSet::new();
            for region in solidcraft_doc::select_profiles(solved, &profiles).map_err(|error| error.to_string())? {
                let canonical =
                    solidcraft_kernel::canonical_revolve_axis(&solved.plane, &region, local_origin, local_dir).map_err(|e| e.to_string())?;
                signs.insert(if canonical.dot(unit) < 0.0 { -1 } else { 1 });
            }
            if signs.len() > 1 {
                return Err("Profiles on both sides of the revolution axis need separate operations.".into());
            }
            let sign = f64::from(signs.first().copied().unwrap_or(1)) * if *reversed { -1.0 } else { 1.0 };
            FeatureKind::Revolve {
                sketch: SketchId(sketch.id),
                profiles,
                axis,
                angle: format!("{} deg", angle * sign).into(),
                angle2: angle2.map(|v| format!("{} deg", v * sign).into()),
                symmetric: *symmetric,
                convention: RevolveConvention::SketchNormal,
                operation: operation(*cut),
                targets: targets.iter().cloned().map(BodyName).collect(),
                to: None,
            }
        }
        FreeCadOperationSettings::Mirror { .. } | FreeCadOperationSettings::PolarPattern { .. } => {
            return Err("A transformed operation has no profile sketch.".into());
        }
    };
    super::super::features::stable_profiles(s, &mut feature, false).map_err(|error| error.to_string())?;
    Ok(feature)
}

#[derive(Clone, Default)]
struct Outcome {
    bodies: Vec<String>,
    blocked: bool,
    source_feature: Option<String>,
}

fn reserve_body_name(used: &mut BTreeSet<String>, base: &str) -> String {
    let mut name = base.to_string();
    let mut suffix = 2;
    while used.contains(&name) {
        name = format!("{base} ({suffix})");
        suffix += 1;
    }
    used.insert(name.clone());
    name
}

pub(super) fn import(s: &mut Session, operations: &[FreeCadOperation], sketches: &BTreeMap<String, ImportedSketch>) -> Result<Vec<Value>> {
    let mut outcomes = BTreeMap::<String, Outcome>::new();
    let by_name: BTreeMap<&str, &FreeCadOperation> = operations.iter().map(|op| (op.source_name.as_str(), op)).collect();
    let mut reports = Vec::new();
    let mut used_bodies: BTreeSet<String> = s.model.state().bodies.iter().map(|body| body.name.clone()).collect();
    let mut source_bodies = BTreeMap::<String, String>::new();
    for op in operations {
        if let Some((source, label)) = &op.body
            && !source_bodies.contains_key(source)
        {
            let base = if label.trim().is_empty() { source } else { label };
            source_bodies.insert(source.clone(), reserve_body_name(&mut used_bodies, base));
        }
    }
    for op in operations {
        let parent = op.base.as_ref().and_then(|base| outcomes.get(base)).cloned();
        let mut outcome = parent.clone().unwrap_or_default();
        outcome.source_feature = None;
        if let Some((source, _)) = &op.body
            && let Some(name) = source_bodies.get(source)
            && (outcome.bodies.is_empty()
                || !op
                    .base
                    .as_deref()
                    .and_then(|base| by_name.get(base))
                    .is_some_and(|base| base.body.as_ref().map(|(name, _)| name) == Some(source)))
        {
            outcome.bodies = vec![name.clone()];
        }
        let mut warnings = op.warnings.clone();
        let mut blockers = op.blockers.clone();
        if let Some(base) = op.base.as_deref().and_then(|name| by_name.get(name))
            && base.body.as_ref().map(|(name, _)| name) != op.body.as_ref().map(|(name, _)| name)
        {
            blockers.push(warning(
                "The preceding operation belongs to another Body.",
                "Cross-Body base-feature relationships require manual repair. Check the imported target and component before enabling this feature.",
            ));
        }
        if op.base.is_some() && parent.as_ref().is_none_or(|parent| parent.blocked) {
            let predecessor = op.base.as_deref().unwrap_or_default();
            let has_target = !outcome.bodies.is_empty() && outcome.bodies.iter().all(|name| s.model.state().body(name).is_some());
            let sketch_based =
                matches!(op.settings.as_ref(), Some(FreeCadOperationSettings::Extrude { .. } | FreeCadOperationSettings::Revolve { .. }));
            if sketch_based && has_target {
                warnings.push(advisory(
                    "The preceding operation is missing; this feature was rebuilt on the available body.",
                    format!(
                        "FreeCAD placed this operation after `{predecessor}`, which was not rebuilt in SolidCraft. The profile operation can still run on the body's current solid. Its result may differ from FreeCAD until `{predecessor}` is rebuilt earlier in the timeline."
                    ),
                ));
            } else {
                blockers.push(warning(
                    "An earlier operation needs repair.",
                    format!(
                        "This operation depends on `{predecessor}`. Resolve its missing or suppressed predecessor before enabling this one. A [Not imported] placeholder records a missing operation that must be rebuilt manually."
                    ),
                ));
            }
        }
        let mut feature_id = None;
        if let Some(settings @ (FreeCadOperationSettings::Mirror { .. } | FreeCadOperationSettings::PolarPattern { .. })) = &op.settings {
            let original = match settings {
                FreeCadOperationSettings::Mirror { original, .. } | FreeCadOperationSettings::PolarPattern { original, .. } => original,
                _ => &op.source_name,
            };
            let source = outcomes
                .get(original)
                .and_then(|outcome| outcome.source_feature.as_deref())
                .and_then(|name| s.doc.find_feature(name))
                .map(|feature| (feature.id.into(), feature.component));
            if let Some((source_feature, component)) = source {
                let source_is_first_pad = by_name.get(original.as_str()).is_some_and(|source| {
                    source.base.is_none() && matches!(source.settings, Some(FreeCadOperationSettings::Extrude { cut: false, .. }))
                });
                let mirror_body = matches!(settings, FreeCadOperationSettings::Mirror { whole_body: true, .. })
                    || matches!(settings, FreeCadOperationSettings::Mirror { .. }) && source_is_first_pad;
                let body = outcome.bodies.first().filter(|name| s.model.state().body(name).is_some()).cloned();
                let body_missing = mirror_body && body.is_none();
                let kind = match settings {
                    FreeCadOperationSettings::Mirror { plane, .. } => {
                        if body_missing {
                            blockers.push(warning(
                                "The mirror's source body is unavailable.",
                                "The preceding operation did not create the body to mirror. Repair that operation before rebuilding this mirror.",
                            ));
                        }
                        Some(FeatureKind::Mirror {
                            features: if mirror_body { Vec::new() } else { vec![source_feature] },
                            plane: PlaneRef::Custom { plane: *plane },
                            bodies: if mirror_body { body.into_iter().map(BodyName).collect() } else { Vec::new() },
                            combine: mirror_body,
                            placements: Vec::new(),
                        })
                    }
                    FreeCadOperationSettings::PolarPattern { origin, axis, count, angle, reversed, .. } => Some(FeatureKind::Pattern {
                        features: vec![source_feature],
                        pattern: PatternKind::Circular {
                            origin: Position(*origin),
                            axis: Dir(*axis),
                            count: count.to_string().into(),
                            angle: format!("{angle} deg").into(),
                            alignment: solidcraft_sketch::PatternAlignment::Right,
                            reversed: *reversed,
                            axis_ref: None,
                            axis_feature: None,
                        },
                        bodies: Vec::new(),
                    }),
                    _ => None,
                };
                if let Some(kind) = kind.filter(|_| !body_missing) {
                    let label = if blockers.is_empty() { op.label.clone() } else { format!("{} [Needs repair]", op.label) };
                    let name = super::freecad_feature_name(s, &label);
                    let before: BTreeSet<String> = s.model.state().bodies.iter().map(|body| body.name.clone()).collect();
                    let id = s.doc_mut().add_feature(kind, Some(&name))?;
                    if let Some(feature) = s.doc_mut().feature_mut(id) {
                        feature.component = component;
                        feature.suppressed = !blockers.is_empty();
                    }
                    s.refresh();
                    if blockers.is_empty() {
                        let created: Vec<String> =
                            s.model.state().bodies.iter().filter(|body| !before.contains(&body.name)).map(|body| body.name.clone()).collect();
                        let error = s
                            .model
                            .result(id)
                            .and_then(|result| result.error.clone())
                            .or_else(|| s.model.result(id).is_none().then(|| "The imported transform has no evaluation result.".into()))
                            .or_else(|| {
                                (!op.body_allows_compound && !created.is_empty()).then(|| {
                                    "The transform created separate solids, but the source FreeCAD Body does not permit a compound result.".into()
                                })
                            });
                        if let Some(error) = error {
                            blockers.push(warning("SolidCraft could not rebuild this operation; it was imported suppressed.", error));
                            if let Some(feature) = s.doc_mut().feature_mut(id) {
                                feature.suppressed = true;
                                feature.name.push_str(" [Needs repair]");
                            }
                            s.refresh();
                        } else {
                            outcome.bodies.extend(created);
                        }
                    }
                    feature_id = Some(id);
                }
            } else {
                blockers.push(warning(
                    "The transformed source feature is unavailable.",
                    format!("FreeCAD source operation `{original}` could not be rebuilt as an active SolidCraft feature. Rebuild it before this mirror or circular pattern."),
                ));
            }
        } else if let (Some(settings), Some(sketch)) = (&op.settings, sketches.get(&op.profile)) {
            if op.body.as_ref().map(|(name, _)| name) != sketch.body.as_ref().map(|(name, _)| name) {
                blockers.push(warning(
                    "The operation uses a profile from another Body.",
                    "Cross-Body profile ownership requires manual repair. Check the feature's component and target body before enabling it.",
                ));
            }
            if sketch.needs_repair {
                let reasons = sketch
                    .warnings
                    .iter()
                    .filter(|issue| issue.summary.starts_with("Geometry ") || issue.summary.contains("attachment"))
                    .map(|issue| format!("{} {}", issue.summary, issue.detail))
                    .collect::<Vec<_>>();
                let detail = format!(
                    "Profile sketch `{}` (`{}`) needs repair. {} The saved plane and available geometry are retained. Redefine the sketch plane or repair the profile, then enable this operation and its dependents.",
                    sketch.label,
                    op.profile,
                    if reasons.is_empty() {
                        "Its source attachment is unresolved or some profile geometry could not be imported.".to_string()
                    } else {
                        reasons.join("\n\n")
                    }
                );
                blockers.push(warning("The profile sketch needs repair before this operation can be enabled.", detail));
            }
            let cut = matches!(settings, FreeCadOperationSettings::Extrude { cut: true, .. } | FreeCadOperationSettings::Revolve { cut: true, .. });
            let targets = if op.base.is_some() || cut {
                if outcome.bodies.is_empty() {
                    blockers.push(warning("The target body could not be identified.", "An explicit unresolved target is retained so enabling this feature cannot accidentally modify other imported bodies. Select the correct target body before enabling it."));
                    vec![format!("Unresolved FreeCAD body ({})", op.source_name)]
                } else {
                    outcome.bodies.clone()
                }
            } else {
                Vec::new()
            };
            for target in &targets {
                if !s.model.state().bodies.iter().any(|body| &body.name == target) {
                    let detail = if cut && op.base.is_none() {
                        format!(
                            "FreeCAD operation `{}` is subtractive and has no preceding BaseFeature in Body `{}`. The source Body's sketches alone do not create a solid, so SolidCraft has nothing to cut. Add or import the missing base solid before enabling this feature and its dependents.",
                            op.source_name,
                            op.body.as_ref().map(|(_, label)| label.as_str()).unwrap_or("unknown"),
                        )
                    } else {
                        format!(
                            "SolidCraft target body `{target}` does not exist in the imported model. Its name may have been reserved by a suppressed or missing predecessor. Repair the preceding operation and confirm this feature's target before enabling it."
                        )
                    };
                    blockers.push(warning("The target body has not been created.", detail));
                }
            }
            match kind(s, sketch, settings, &targets, op.base.is_some()) {
                Ok(kind) => {
                    let label = if blockers.is_empty() { op.label.clone() } else { format!("{} [Needs repair]", op.label) };
                    let name = super::freecad_feature_name(s, &label);
                    let id = s.doc_mut().add_feature(kind, Some(&name))?;
                    let new_body = !cut && op.base.is_none();
                    let body_name = if new_body {
                        if let Some((source, _)) = &op.body {
                            source_bodies.get(source).cloned()
                        } else {
                            let base = sketch.body.as_ref().map(|(_, label)| label.as_str()).unwrap_or(&op.label);
                            Some(reserve_body_name(&mut used_bodies, base))
                        }
                    } else {
                        None
                    };
                    if let Some(feature) = s.doc_mut().feature_mut(id) {
                        feature.component = sketch.component;
                        feature.suppressed = !blockers.is_empty();
                        if let Some(name) = &body_name {
                            feature.body_names = vec![name.clone()];
                        }
                    }
                    s.doc_mut().name_feature_inputs(id);
                    let before: BTreeSet<String> = s.model.state().bodies.iter().map(|body| body.name.clone()).collect();
                    s.refresh();
                    if blockers.is_empty() {
                        let error = match s.model.result(id) {
                            Some(result) => result.error.clone(),
                            None => Some("The imported feature has no evaluation result.".into()),
                        }
                        .or_else(|| {
                            body_name.as_ref().filter(|name| !s.model.state().bodies.iter().any(|body| body.name == name.as_str())).map(|name| {
                                format!("The feature evaluated without creating its expected SolidCraft body `{name}`. Check the profile's closed regions, plane and extent.")
                            })
                        });
                        if let Some(error) = error {
                            blockers.push(warning("SolidCraft could not rebuild this operation; it was imported suppressed.", error));
                            if let Some(feature) = s.doc_mut().feature_mut(id) {
                                feature.suppressed = true;
                                feature.name.push_str(" [Needs repair]");
                            }
                            s.refresh();
                        } else {
                            outcome.bodies = s
                                .model
                                .state()
                                .bodies
                                .iter()
                                .filter(|body| targets.contains(&body.name) || !before.contains(&body.name))
                                .map(|body| body.name.clone())
                                .collect();
                        }
                    }
                    if !blockers.is_empty()
                        && let Some(name) = body_name
                    {
                        outcome.bodies = vec![name];
                    }
                    feature_id = Some(id);
                }
                Err(error) => blockers.push(warning("This operation could not be represented as a native feature.", error)),
            }
        } else if op.settings.is_some() {
            blockers.push(warning(
                "The required profile sketch was not available.",
                format!(
                    "FreeCAD operation `{}` refers to profile `{}`. The profile could not be imported as an editable sketch.",
                    op.source_name, op.profile
                ),
            ));
        }
        let skipped = feature_id.is_none();
        outcome.blocked = !blockers.is_empty() || skipped;
        if !outcome.blocked {
            outcome.source_feature = feature_id.and_then(|id| s.doc.feature(id).map(|feature| feature.name.clone()));
        }
        warnings.extend(blockers);
        let mut context = format!("FreeCAD operation: `{}` (`{}`, {}).", op.label, op.source_name, op.type_name);
        if let Some((source, label)) = &op.body {
            context.push_str(&format!(" FreeCAD Body: `{label}` (`{source}`)."));
        }
        if !op.profile.is_empty() {
            if let Some(sketch) = sketches.get(&op.profile) {
                context.push_str(&format!(" Profile sketch: `{}` (`{}`).", sketch.label, op.profile));
            } else {
                context.push_str(&format!(" Profile sketch: `{}` (unavailable).", op.profile));
            }
        }
        if let Some(base) = &op.base {
            if let Some(predecessor) = by_name.get(base.as_str()) {
                context.push_str(&format!(" Preceding operation: `{}` (`{}`).", predecessor.label, base));
                if !predecessor.profile.is_empty() {
                    if let Some(profile) = sketches.get(&predecessor.profile) {
                        context.push_str(&format!(" Preceding profile sketch: `{}` (`{}`).", profile.label, predecessor.profile));
                    } else {
                        context.push_str(&format!(" Preceding profile sketch: `{}` (unavailable).", predecessor.profile));
                    }
                }
            } else {
                context.push_str(&format!(" Preceding operation: `{base}` (unavailable)."));
            }
        }
        if !outcome.bodies.is_empty() {
            context.push_str(&format!(
                " Intended SolidCraft body target: {}.",
                outcome.bodies.iter().map(|name| format!("`{name}`")).collect::<Vec<_>>().join(", ")
            ));
        }
        for issue in &mut warnings {
            issue.detail.push_str("\n\nRelated objects: ");
            issue.detail.push_str(&context);
        }
        if skipped {
            let mut detail = format!(
                "Source operation: {} ({})\n\nThis placeholder records a missing operation. It creates no geometry and cannot be enabled to recreate the original operation. Rebuild it manually to recover the source shape. Later features may have been built on the body without it.\n\n",
                op.label, op.source_name
            );
            for issue in &warnings {
                detail.push_str(&format!("{}\n{}\n\n", issue.summary, issue.detail));
            }
            let component = op
                .body
                .as_ref()
                .and_then(|(body, _)| sketches.values().find(|sketch| sketch.body.as_ref().is_some_and(|(source, _)| source == body)))
                .or_else(|| sketches.get(&op.profile))
                .map(|sketch| sketch.component)
                .unwrap_or(s.active_component);
            let name = super::freecad_feature_name(s, &format!("[Not imported] {}", op.label));
            let id = s.doc_mut().add_feature(
                FeatureKind::ImportPlaceholder { source: op.source_name.clone(), source_type: op.type_name.clone(), detail },
                Some(&name),
            )?;
            if let Some(feature) = s.doc_mut().feature_mut(id) {
                feature.component = component;
            }
            feature_id = Some(id);
            s.refresh();
        }
        reports.push(json!({"source": op.source_name, "name": op.label, "type": op.type_name, "feature": feature_id,
            "status": if skipped { "skipped" } else if outcome.blocked { "needs_repair" } else { "imported" }, "warnings": warnings}));
        outcomes.insert(op.source_name.clone(), outcome);
        used_bodies.extend(s.model.state().bodies.iter().map(|body| body.name.clone()));
    }
    if !operations.is_empty() {
        let mut saved = reports.iter().take(200).cloned().collect::<Vec<_>>();
        while serde_json::to_vec(&saved).map_or(usize::MAX, |bytes| bytes.len()) > 60_000 {
            if saved.pop().is_none() {
                break;
            }
        }
        let omitted = reports.len().saturating_sub(saved.len());
        s.doc_mut().metadata.set(
            solidcraft_doc::metadata::Owner::Design,
            "org.solidcraft.freecad.operations",
            json!({"last_import": saved, "omitted": omitted}),
        )?;
    }
    Ok(reports)
}
