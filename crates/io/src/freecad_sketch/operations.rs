//! Clean-room conversion of saved Part Design operation properties.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FreeCadAxis {
    SketchX,
    SketchY,
    SketchLine { geometry: usize },
    Line { origin: Vec3, dir: Vec3 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FreeCadOperationSettings {
    Extrude { length: f64, length2: Option<f64>, symmetric: bool, reversed: bool, cut: bool, through_all: bool, taper: f64 },
    Revolve { angle: f64, angle2: Option<f64>, symmetric: bool, reversed: bool, cut: bool, axis: FreeCadAxis },
    Mirror { plane: Plane, original: String, whole_body: bool },
    PolarPattern { origin: Vec3, axis: Vec3, count: usize, angle: f64, reversed: bool, original: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FreeCadOperation {
    pub source_name: String,
    pub label: String,
    pub type_name: String,
    pub profile: String,
    pub base: Option<String>,
    pub body: Option<(String, String)>,
    /// Whether the source Part Design Body permits disconnected solids.
    #[serde(default)]
    pub body_allows_compound: bool,
    pub settings: Option<FreeCadOperationSettings>,
    pub warnings: Vec<FreeCadWarning>,
    /// Conditions requiring repair before evaluating this operation.
    pub blockers: Vec<FreeCadWarning>,
}

pub(super) fn capture(object: &mut RawObject, tag: &[u8], property: &str, attrs: &BTreeMap<String, String>) {
    if tag == b"Expression" {
        object.expressions = true;
    }
    if !matches!(
        property,
        "Type"
            | "SideType"
            | "Length"
            | "Length2"
            | "Angle"
            | "Angle2"
            | "TaperAngle"
            | "TaperAngle2"
            | "Reversed"
            | "Midplane"
            | "AlongSketchNormal"
            | "Profile"
            | "BaseFeature"
            | "ReferenceAxis"
            | "Origin"
            | "Suppressed"
            | "Base"
            | "Radius"
            | "UseAllEdges"
            | "Occurrences"
            | "Mode"
            | "Offset"
            | "Axis"
            | "MirrorPlane"
            | "Originals"
            | "TransformMode"
            | "Refine"
            | "AllowCompound"
    ) {
        return;
    }
    match tag {
        b"Float" | b"Integer" | b"Bool" => {
            object.values.insert(property.into(), string(attrs, "value"));
        }
        b"Link" if property == "Originals" => {
            let name = string(attrs, "value");
            if !name.is_empty() {
                object.pattern_originals.push(name);
            }
        }
        b"Link" | b"LinkSub" => {
            object.links.insert(property.into(), (string(attrs, "value"), Vec::new()));
        }
        b"Sub" => {
            if let Some((_, subs)) = object.links.get_mut(property) {
                subs.push(string(attrs, "value"));
            }
        }
        _ => {}
    }
}

fn issue(summary: impl Into<String>, detail: impl Into<String>) -> FreeCadWarning {
    FreeCadWarning { summary: summary.into(), detail: detail.into(), severity: FreeCadWarningSeverity::Issue, context: None }
}

fn value(object: &RawObject, property: &str, default: f64) -> Option<f64> {
    match object.values.get(property) {
        Some(raw) => raw.parse::<f64>().ok().filter(|v| v.is_finite() && v.abs() <= 1e9),
        None => default.is_finite().then_some(default),
    }
}

fn flag(object: &RawObject, property: &str, default: bool) -> bool {
    object.values.get(property).map_or(default, |v| v == "true" || v == "1")
}

fn fillet_details(object: &RawObject, objects: &HashMap<&str, &RawObject>) -> String {
    let radius = value(object, "Radius", f64::NAN).map_or_else(|| "unavailable".to_string(), |radius| format!("{radius} mm"));
    let Some((base, selected)) = object.links.get("Base") else {
        return format!("Saved fillet radius: {radius}. The FreeCAD base operation and selected edges are unavailable.");
    };
    let label = objects.get(base.as_str()).map(|source| source.label.as_str()).filter(|label| !label.is_empty()).unwrap_or(base);
    let mut detail = format!("Saved fillet radius: {radius}. FreeCAD base operation: `{label}` (`{base}`). ");
    if flag(object, "UseAllEdges", false) {
        detail.push_str("FreeCAD applied the fillet to all edges of that base operation.");
    } else if selected.is_empty() {
        detail.push_str("The file does not list selected edges or faces for this fillet.");
    } else {
        let edges: Vec<_> = selected.iter().filter(|name| name.starts_with("Edge")).map(String::as_str).collect();
        let faces: Vec<_> = selected.iter().filter(|name| name.starts_with("Face")).map(String::as_str).collect();
        let other: Vec<_> = selected.iter().filter(|name| !name.starts_with("Edge") && !name.starts_with("Face")).map(String::as_str).collect();
        if !edges.is_empty() {
            detail.push_str(&format!("Selected FreeCAD edges: {}. ", edges.iter().map(|name| format!("`{name}`")).collect::<Vec<_>>().join(", ")));
        }
        if !faces.is_empty() {
            detail.push_str(&format!(
                "Selected FreeCAD faces: {}. The file does not provide a separate edge list for this face selection. ",
                faces.iter().map(|name| format!("`{name}`")).collect::<Vec<_>>().join(", ")
            ));
        }
        if !other.is_empty() {
            detail
                .push_str(&format!("Other selected subelements: {}. ", other.iter().map(|name| format!("`{name}`")).collect::<Vec<_>>().join(", ")));
        }
        detail.push_str(
            "These identifiers belong to the FreeCAD source model; select the corresponding geometry when rebuilding the fillet in SolidCraft.",
        );
    }
    detail
}

fn source_object(name: &str, objects: &HashMap<&str, &RawObject>) -> String {
    let label = objects.get(name).map(|source| source.label.as_str()).filter(|label| !label.is_empty()).unwrap_or(name);
    if label == name { format!("`{name}`") } else { format!("`{label}` (`{name}`)") }
}

fn polar_pattern_details(object: &RawObject, objects: &HashMap<&str, &RawObject>) -> String {
    let mut details = Vec::new();
    let occurrences = object.values.get("Occurrences").and_then(|raw| raw.parse::<usize>().ok()).filter(|count| *count > 0);
    details.push(match occurrences {
        Some(count) => format!("Saved occurrence count: {count}, including the original feature."),
        None => "Saved occurrence count: unavailable.".into(),
    });
    let mode = object.values.get("Mode").map(String::as_str).unwrap_or("0");
    match mode {
        "0" => {
            if let Some(angle) = value(object, "Angle", f64::NAN) {
                details.push(format!("Total pattern angle: {angle}° (overall angle mode)."));
                if let Some(count) = occurrences.filter(|count| *count > 1)
                    && angle > 0.0
                {
                    let step = if (angle - 360.0).abs() < 1e-8 { angle / count as f64 } else { angle / (count - 1) as f64 };
                    let step = format!("{step:.6}");
                    details.push(format!("Nominal angular step: {}°.", step.trim_end_matches('0').trim_end_matches('.')));
                }
            } else {
                details.push("Total pattern angle: unavailable.".into());
            }
        }
        "1" => {
            details.push("Angle mode: spacing between occurrences.".into());
            details.push(
                value(object, "Offset", f64::NAN).map_or_else(
                    || "Saved offset angle: unavailable.".into(),
                    |offset| format!("Saved offset angle: {offset}° between occurrences."),
                ),
            );
            details.push(
                "The total span may depend on separate per-instance spacing values, so the saved overall Angle property is not treated as active."
                    .into(),
            );
        }
        other => details.push(format!("Angle mode: unrecognized saved value `{other}`; total pattern angle cannot be identified reliably.")),
    }
    if let Some((name, subs)) = object.links.get("Axis") {
        let source = source_object(name, objects);
        let axis = match subs.first().map(String::as_str) {
            Some("N_Axis") => format!("normal (Z) axis of {source}"),
            Some("V_Axis") => format!("vertical (Y) axis of {source}"),
            Some("H_Axis") => format!("horizontal (X) axis of {source}"),
            Some(sub) if !sub.is_empty() => format!("{source}, subelement `{sub}`"),
            _ => source,
        };
        details.push(format!("Reference axis: {axis}."));
    } else {
        details.push("Reference axis: unavailable.".into());
    }
    if object.pattern_originals.is_empty() {
        details.push("Patterned source features: unavailable.".into());
    } else {
        details.push(format!(
            "Patterned source features: {}.",
            object.pattern_originals.iter().map(|name| source_object(name, objects)).collect::<Vec<_>>().join(", ")
        ));
    }
    details.push(format!("Direction: {}.", if flag(object, "Reversed", false) { "reversed" } else { "normal" }));
    if let Some(mode) = object.values.get("TransformMode") {
        let mode = match mode.as_str() {
            "0" => "selected features".to_string(),
            "1" => "whole base shape".to_string(),
            other => format!("unrecognized saved value `{other}`"),
        };
        details.push(format!("Transform mode: {mode}."));
    }
    if let Some(refine) = object.values.get("Refine") {
        details.push(format!("Refine result: {}.", if refine == "true" || refine == "1" { "yes" } else { "no" }));
    }
    if flag(object, "Suppressed", false) {
        details.push("The source pattern was suppressed in FreeCAD.".into());
    }
    if object.expressions {
        details.push("The values above are saved numeric results; source expressions are not translated.".into());
    }
    details.join("\n")
}

fn axis(object: &RawObject, objects: &HashMap<&str, &RawObject>, parents: &HashMap<&str, &str>, profile: &str) -> Option<FreeCadAxis> {
    let (name, subs) = object.links.get("ReferenceAxis")?;
    let sub = subs.first().map(String::as_str).unwrap_or("");
    if name == profile {
        return match sub {
            "H_Axis" => Some(FreeCadAxis::SketchX),
            "V_Axis" => Some(FreeCadAxis::SketchY),
            _ => sub
                .strip_prefix("Edge")
                .and_then(|index| index.parse::<usize>().ok())
                .and_then(|index| index.checked_sub(1))
                .map(|geometry| FreeCadAxis::SketchLine { geometry }),
        };
    }
    let target = objects.get(name.as_str())?;
    if target.type_name == "App::Line" && sub.is_empty() {
        let frame = global_frame(name, objects, parents)?;
        return Some(FreeCadAxis::Line { origin: frame.origin, dir: frame.x });
    }
    None
}

fn mirror_plane(object: &RawObject, objects: &HashMap<&str, &RawObject>, parents: &HashMap<&str, &str>) -> Option<Plane> {
    let (name, subs) = object.links.get("MirrorPlane")?;
    let target = objects.get(name.as_str())?;
    let frame = global_frame(name, objects, parents)?;
    let sub = subs.first().map(String::as_str).unwrap_or("");
    match (target.type_name.as_str(), sub) {
        ("App::Plane" | "PartDesign::Plane", "") => Some(frame),
        ("Sketcher::SketchObject", "N_Axis") => Some(frame),
        ("Sketcher::SketchObject", "H_Axis") => Plane::new(frame.origin, frame.x, frame.normal()),
        ("Sketcher::SketchObject", "V_Axis") => Plane::new(frame.origin, frame.y, frame.normal()),
        _ => None,
    }
}

fn pattern_axis(object: &RawObject, objects: &HashMap<&str, &RawObject>, parents: &HashMap<&str, &str>) -> Option<(Vec3, Vec3)> {
    let (name, subs) = object.links.get("Axis")?;
    let target = objects.get(name.as_str())?;
    let frame = global_frame(name, objects, parents)?;
    let sub = subs.first().map(String::as_str).unwrap_or("");
    let dir = match (target.type_name.as_str(), sub) {
        ("Sketcher::SketchObject", "N_Axis") => frame.normal(),
        ("Sketcher::SketchObject", "H_Axis") => frame.x,
        ("Sketcher::SketchObject", "V_Axis") => frame.y,
        ("App::Line" | "PartDesign::Line", "") => frame.x,
        _ => return None,
    };
    Some((frame.origin, dir))
}

pub(super) fn collect(objects: &[RawObject], by_name: &HashMap<&str, &RawObject>, parents: &HashMap<&str, &str>) -> Vec<FreeCadOperation> {
    objects.iter().filter(|object| (object.type_name.starts_with("PartDesign::") &&
        !matches!(object.type_name.as_str(), "PartDesign::Body" | "PartDesign::Plane" | "PartDesign::Line" | "PartDesign::Point"))
        || matches!(object.type_name.as_str(), "Part::Extrusion" | "Part::Revolution"))
        .map(|object| {
            let profile = object.links.get("Profile").map(|(name, _)| name.clone()).unwrap_or_default();
            let base = object.links.get("BaseFeature").map(|(name, _)| name.clone()).filter(|name| !name.is_empty());
            let mut owner = parents.get(object.source_name.as_str()).copied();
            let mut body = None;
            for _ in 0..64 {
                let Some(name) = owner else { break };
                if let Some(parent) = by_name.get(name) && parent.type_name == "PartDesign::Body" {
                    body = Some((parent.source_name.clone(), parent.label.clone()));
                    break;
                }
                owner = parents.get(name).copied();
            }
            let mut operation = FreeCadOperation {
                source_name: object.source_name.clone(), label: object.label.clone(), type_name: object.type_name.clone(),
                profile, base, body_allows_compound: body.as_ref().and_then(|(name, _)| by_name.get(name.as_str())).is_some_and(|body| flag(body, "AllowCompound", false)),
                body, settings: None, warnings: Vec::new(), blockers: Vec::new(),
            };
            if matches!(object.type_name.as_str(), "PartDesign::Mirrored" | "PartDesign::PolarPattern") {
                let original = match (object.pattern_originals.as_slice(), operation.base.as_deref()) {
                    ([original], Some(base)) if original == base => Some(original.clone()),
                    _ => {
                        let mut detail = "This importer can rebuild a mirror or circular pattern when it transforms exactly its preceding feature. The saved Originals list is empty, contains multiple features, or differs from BaseFeature.".to_string();
                        if object.type_name == "PartDesign::PolarPattern" {
                            detail.push_str("\n\n");
                            detail.push_str(&polar_pattern_details(object, by_name));
                        }
                        operation.blockers.push(issue(
                            "The transformed source features need manual repair.",
                            detail,
                        ));
                        None
                    }
                };
                let transform_mode = object.values.get("TransformMode").map(String::as_str).unwrap_or("0");
                if flag(object, "Suppressed", false) {
                    operation.blockers.push(issue("This operation was suppressed in FreeCAD.", "The imported native feature remains suppressed. Review it before enabling it in SolidCraft."));
                }
                if object.expressions {
                    operation.warnings.push(issue("Operation expressions use their saved numeric values.", "FreeCAD formulas and spreadsheet references are not translated yet. The imported operation's numeric values remain editable in SolidCraft."));
                }
                if let Some(original) = original {
                    match object.type_name.as_str() {
                        "PartDesign::Mirrored" if matches!(transform_mode, "0" | "1") => {
                            operation.settings = mirror_plane(object, by_name, parents)
                                .map(|plane| FreeCadOperationSettings::Mirror { plane, original, whole_body: transform_mode == "1" });
                            if operation.settings.is_none() {
                                operation.blockers.push(issue("The mirror plane could not be identified.", "Supported source planes are saved origin or datum planes and a sketch's horizontal, vertical or normal axis. Rebuild the mirror in SolidCraft using the corresponding plane."));
                            }
                        }
                        "PartDesign::PolarPattern" if transform_mode == "0" => {
                            let count = object.values.get("Occurrences").and_then(|raw| raw.parse::<usize>().ok()).filter(|count| (2..=1000).contains(count));
                            let angle = (object.values.get("Mode").map(String::as_str).unwrap_or("0") == "0")
                                .then(|| value(object, "Angle", f64::NAN))
                                .flatten()
                                .filter(|angle| *angle > 0.0 && *angle <= 360.0);
                            operation.settings = match (pattern_axis(object, by_name, parents), count, angle) {
                                (Some((origin, axis)), Some(count), Some(angle)) => Some(FreeCadOperationSettings::PolarPattern {
                                    origin, axis, count, angle, reversed: flag(object, "Reversed", false), original,
                                }),
                                _ => None,
                            };
                            if operation.settings.is_none() {
                                operation.blockers.push(issue("The circular pattern parameters need manual repair.", format!(
                                    "A saved sketch or datum axis, 2–1000 occurrences and an overall angle above 0° through 360° are required. {}",
                                    polar_pattern_details(object, by_name),
                                )));
                            }
                        }
                        _ => {
                            let mut detail = format!("FreeCAD TransformMode `{transform_mode}` is not mapped to this SolidCraft feature. The source operation is kept as a placeholder.");
                            if object.type_name == "PartDesign::PolarPattern" {
                                detail.push_str("\n\n");
                                detail.push_str(&polar_pattern_details(object, by_name));
                            }
                            operation.blockers.push(issue("The transform mode needs manual repair.", detail));
                        }
                    }
                }
                return operation;
            }
            if !matches!(object.type_name.as_str(), "PartDesign::Pad" | "PartDesign::Pocket" | "PartDesign::Revolution" | "PartDesign::Groove") {
                let mut detail = format!("FreeCAD operation `{}` has type `{}`. This importer currently rebuilds Pad, Pocket, Revolution, Groove, Mirrored and PolarPattern when their saved references are supported. Later operations may rebuild on the available body, but their shape can differ until this operation is restored.", object.source_name, object.type_name);
                if object.type_name == "PartDesign::Fillet" {
                    detail.push_str("\n\n");
                    detail.push_str(&fillet_details(object, by_name));
                } else if object.type_name == "PartDesign::PolarPattern" {
                    detail.push_str("\n\n");
                    detail.push_str(&polar_pattern_details(object, by_name));
                }
                operation.blockers.push(issue("This operation is not supported yet.", detail));
                return operation;
            }
            let cut = matches!(object.type_name.as_str(), "PartDesign::Pocket" | "PartDesign::Groove");
            let revolve = matches!(object.type_name.as_str(), "PartDesign::Revolution" | "PartDesign::Groove");
            let mode = value(object, "Type", 0.0).unwrap_or(-1.0);
            let side = value(object, "SideType", if flag(object, "Midplane", false) { 2.0 } else if mode == 4.0 { 1.0 } else { 0.0 }).unwrap_or(-1.0);
            let symmetric = side == 2.0;
            let reversed = flag(object, "Reversed", false);
            let through_all = !revolve && cut && mode == 1.0;
            if flag(object, "Suppressed", false) {
                operation.blockers.push(issue("This operation was suppressed in FreeCAD.", "The imported native feature remains suppressed. Review it before enabling it in SolidCraft."));
            }
            if !(mode == 0.0 || mode == 4.0 || through_all) || ![0.0, 1.0, 2.0].contains(&side) {
                operation.blockers.push(issue("The operation's extent requires manual repair.", format!("FreeCAD extent mode {mode}, side mode {side}. Fixed length/angle, symmetric, two-sided and through-all Pocket are supported. Up-to-face, up-to-first and other reference extents are preserved as suppressed fixed extents using the saved numeric values; edit them before enabling.")));
            }
            if object.links.get("Profile").is_some_and(|(_, subs)| subs.iter().any(|sub| !sub.is_empty())) {
                operation.blockers.push(issue("The selected profile subelements require repair.", "This operation selects individual source profile subelements. The imported feature uses the sketch's complete profiles and is suppressed until its profile selection is repaired."));
            }
            if object.expressions {
                operation.warnings.push(issue("Operation expressions use their saved numeric values.", "FreeCAD formulas and spreadsheet references are not translated yet. The imported operation's numeric dimensions remain editable in SolidCraft."));
            }
            operation.settings = if revolve {
                let angle = value(object, "Angle", f64::NAN);
                let angle2 = if side == 1.0 { value(object, "Angle2", f64::NAN) } else { Some(0.0) };
                match (angle, angle2, axis(object, by_name, parents, &operation.profile)) {
                    (Some(angle), Some(second), Some(axis)) if angle > 0.0 && angle <= 360.0 && second >= 0.0 && angle + second <= 360.0 + 1e-8 =>
                        Some(FreeCadOperationSettings::Revolve { angle, angle2: (side == 1.0).then_some(second), symmetric, reversed, cut, axis }),
                    _ => {
                        operation.blockers.push(issue("The revolution axis or angle could not be imported.", "Supported axes are the profile's horizontal/vertical axis, a line in that sketch, or an origin axis. The angle must be finite and the total sweep at most 360 degrees. No substitute axis or angle is invented."));
                        None
                    }
                }
            } else {
                if !flag(object, "AlongSketchNormal", true) {
                    operation.blockers.push(issue("The custom extrusion direction requires repair.", "The imported feature is suppressed and uses the sketch normal. Edit the sketch plane or feature direction to reproduce the source operation before enabling it."));
                }
                let length = value(object, "Length", if through_all { 1.0 } else { f64::NAN });
                let length2 = if side == 1.0 { value(object, "Length2", f64::NAN) } else { Some(0.0) };
                let taper = value(object, "TaperAngle", 0.0);
                if taper.is_some_and(|angle| angle.abs() > 1e-9) || value(object, "TaperAngle2", 0.0).is_some_and(|angle| angle.abs() > 1e-9) {
                    operation.blockers.push(issue("The tapered extrusion requires a direction check.", "The saved taper angle is retained, but FreeCAD and SolidCraft can use different taper-side conventions. Check the angle and result before enabling this suppressed feature."));
                }
                // FreeCAD accepts a negative one-sided length and extrudes in the
                // opposite direction even when its Reversed property is false.
                let negative_length = mode == 0.0 && side == 0.0 && length.is_some_and(|value| value < 0.0);
                let length = length.map(|value| if negative_length { -value } else { value });
                let reversed = reversed != negative_length;
                match (length, length2, taper) {
                    (Some(first), Some(second), Some(taper)) if side == 1.0 && (first == 0.0 || first == -1.0) && second > 0.0 =>
                        Some(FreeCadOperationSettings::Extrude { length: second, length2: None, symmetric: false, reversed: !reversed, cut, through_all, taper }),
                    (Some(length), Some(second), Some(taper)) if (length > 0.0 || through_all) && second >= 0.0 =>
                        Some(FreeCadOperationSettings::Extrude { length, length2: (side == 1.0).then_some(second), symmetric, reversed, cut, through_all, taper }),
                    _ => {
                        operation.blockers.push(issue("The extrusion dimensions are invalid or unavailable.", "The saved lengths or taper are missing, non-finite or outside SolidCraft's supported range."));
                        None
                    }
                }
            };
            operation
        }).collect()
}
