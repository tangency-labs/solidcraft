//! Editable sketches from FreeCAD `.FCStd` archives.
//!
//! This is a clean-room reader of the public ZIP/XML interchange representation. It does not
//! link to FreeCAD or use its source. Origin and datum plane placements are recovered from saved
//! frames; unsupported attachments are reported alongside geometry and constraint losses.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::{Deserialize, Serialize};
use solidcraft_geom::{Plane, Vec2, Vec3};
use solidcraft_sketch::{ConstraintKind, CurveKind, Sketch};

use crate::{IoError, Result};

mod operations;
pub use operations::{FreeCadAxis, FreeCadOperation, FreeCadOperationSettings};

/// Largest uncompressed `Document.xml` accepted from an FCStd archive.
pub const MAX_FREECAD_DOCUMENT_XML: u64 = 64 << 20;
const MAX_XML_DEPTH: usize = 128;
const MAX_XML_EVENTS: usize = 1_000_000;
const MAX_OBJECTS: usize = 50_000;
const MAX_WARNINGS: usize = 200;

/// A source-project item shown in the sketch chooser.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeCadNode {
    pub source_name: String,
    pub label: String,
    pub kind: FreeCadNodeKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<FreeCadNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch: Option<FreeCadSketchInfo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreeCadNodeKind {
    Part,
    Body,
    Sketch,
}

/// Counts and losses shown before a sketch is selected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeCadSketchInfo {
    pub geometry: usize,
    pub constraints: usize,
    #[serde(default)]
    pub plane: String,
    #[serde(default)]
    pub saved_plane_available: bool,
    #[serde(default)]
    pub attachment_unresolved: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<FreeCadWarning>,
}

/// A concise import warning with optional technical context for the UI's More dialog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeCadWarning {
    pub summary: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "FreeCadWarningSeverity::is_issue")]
    pub severity: FreeCadWarningSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<FreeCadIssueContext>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreeCadWarningSeverity {
    #[default]
    Issue,
    Warning,
}

impl FreeCadWarningSeverity {
    fn is_issue(&self) -> bool {
        *self == Self::Issue
    }
}

/// Source references retained for diagnostic pictures, independent of translated constraints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeCadIssueContext {
    pub sketch: String,
    /// (FreeCAD geometry index, point position); -1/-2 are the sketch axes.
    pub references: Vec<(i32, i32)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FreeCadPreviewCurve {
    pub geometry: usize,
    pub points: Vec<Vec2>,
    pub construction: bool,
    pub start: Option<Vec2>,
    pub end: Option<Vec2>,
    pub center: Option<Vec2>,
}

/// Bounded saved geometry for a report; never substitutes for the imported model.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FreeCadSketchPreview {
    pub source: String,
    pub curves: Vec<FreeCadPreviewCurve>,
    pub omitted: usize,
}

/// One converted sketch and its source identity.
#[derive(Clone, Debug)]
pub struct FreeCadSketch {
    pub source_name: String,
    pub label: String,
    pub path: Vec<String>,
    /// FreeCAD Part source-name/label pairs, outermost first.
    pub parts: Vec<(String, String)>,
    /// The nearest FreeCAD Body source-name/label pair, when the sketch belongs to one.
    pub body: Option<(String, String)>,
    pub plane: Plane,
    /// A shared datum plane, expressed in source-project coordinates.
    pub datum: Option<(String, String, Plane)>,
    pub plane_description: String,
    pub saved_plane: Option<Plane>,
    pub attachment_unresolved: bool,
    pub source_curves: Vec<Option<String>>,
    pub preview: FreeCadSketchPreview,
    pub sketch: Sketch,
    pub warnings: Vec<FreeCadWarning>,
}

/// The selectable hierarchy and converted sketches in an FCStd file.
#[derive(Clone, Debug)]
pub struct FreeCadProject {
    pub label: String,
    pub nodes: Vec<FreeCadNode>,
    pub sketches: Vec<FreeCadSketch>,
    pub operations: Vec<FreeCadOperation>,
}

#[derive(Clone, Debug, Default)]
struct RawObject {
    source_name: String,
    type_name: String,
    label: String,
    children: Vec<String>,
    geometry: Vec<RawGeometry>,
    constraints: Vec<RawConstraint>,
    /// None means absent; Some(None) means malformed.
    placement: Option<Option<Plane>>,
    support: Vec<(String, String)>,
    map_mode: i32,
    values: BTreeMap<String, String>,
    links: BTreeMap<String, (String, Vec<String>)>,
    pattern_originals: Vec<String>,
    expressions: bool,
}

#[derive(Clone, Debug)]
struct RawGeometry {
    type_name: String,
    construction: bool,
    data: GeometryData,
}

#[derive(Clone, Debug, Default)]
enum GeometryData {
    Point {
        position: Vec2,
    },
    Line {
        start: Vec2,
        end: Vec2,
    },
    Circle {
        center: Vec2,
        radius: f64,
    },
    Arc {
        center: Vec2,
        radius: f64,
        start: f64,
        end: f64,
        angle: f64,
    },
    Ellipse {
        center: Vec2,
        major: f64,
        minor: f64,
        angle: f64,
    },
    #[default]
    Unsupported,
}

#[derive(Clone, Debug, Default)]
struct RawConstraint {
    kind: i32,
    value: f64,
    first: i32,
    first_pos: i32,
    second: i32,
    second_pos: i32,
    third: i32,
    third_pos: i32,
    driving: bool,
    active: bool,
    virtual_space: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct GeometryMap {
    curve: Option<usize>,
    start: Option<usize>,
    end: Option<usize>,
    center: Option<usize>,
}

/// Read the editable sketches in a FreeCAD archive. Geometry or constraints not understood are
/// skipped per sketch and reported; a damaged archive or `Document.xml` is an error.
pub fn read_freecad_sketches(bytes: &[u8], file: &str) -> Result<FreeCadProject> {
    if bytes.len() > crate::MAX_STEP_BYTES {
        return Err(IoError::Invalid(format!("{file}: file is larger than {} MB", crate::MAX_STEP_BYTES >> 20)));
    }
    let xml = document_xml(bytes, file)?;
    parse_document(&xml, file)
}

fn document_xml(bytes: &[u8], file: &str) -> Result<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| IoError::Invalid(format!("{file}: not a valid FCStd archive: {e}")))?;
    let index = (0..zip.len())
        .find(|i| zip.name_for_index(*i).and_then(|n| n.ok()).is_some_and(|n| n.trim_start_matches('/').eq_ignore_ascii_case("Document.xml")))
        .ok_or_else(|| IoError::Invalid(format!("{file}: FCStd archive has no Document.xml")))?;
    let mut entry = zip.by_index(index).map_err(|e| IoError::Invalid(format!("{file}: could not read Document.xml: {e}")))?;
    if entry.size() > MAX_FREECAD_DOCUMENT_XML {
        return Err(IoError::Invalid(format!("{file}: Document.xml expands past {} MB", MAX_FREECAD_DOCUMENT_XML >> 20)));
    }
    let mut xml = Vec::with_capacity(entry.size().min(4 << 20) as usize);
    (&mut entry)
        .take(MAX_FREECAD_DOCUMENT_XML.saturating_add(1))
        .read_to_end(&mut xml)
        .map_err(|e| IoError::Invalid(format!("{file}: could not read Document.xml: {e}")))?;
    if xml.len() as u64 > MAX_FREECAD_DOCUMENT_XML {
        return Err(IoError::Invalid(format!("{file}: Document.xml expands past {} MB", MAX_FREECAD_DOCUMENT_XML >> 20)));
    }
    Ok(xml)
}

fn attrs(e: &BytesStart<'_>) -> BTreeMap<String, String> {
    e.attributes()
        .flatten()
        .map(|a| {
            let key = String::from_utf8_lossy(a.key.local_name().as_ref()).to_string();
            let value = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|v| v.to_string())
                .unwrap_or_else(|_| String::from_utf8_lossy(&a.value).to_string());
            (key, value)
        })
        .collect()
}

fn string(a: &BTreeMap<String, String>, key: &str) -> String {
    a.get(key).cloned().unwrap_or_default()
}

fn number(a: &BTreeMap<String, String>, key: &str) -> f64 {
    a.get(key).and_then(|v| v.parse().ok()).filter(|v: &f64| v.is_finite()).unwrap_or(0.0)
}

fn integer(a: &BTreeMap<String, String>, key: &str, default: i32) -> i32 {
    a.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn parse_document(xml: &[u8], file: &str) -> Result<FreeCadProject> {
    let mut reader = Reader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut depth = 0usize;
    let mut events = 0usize;
    let mut in_objects = false;
    let mut in_object_data = false;
    let mut types: HashMap<String, String> = HashMap::new();
    let mut objects: Vec<RawObject> = Vec::new();
    let mut current: Option<RawObject> = None;
    let mut property = String::new();
    let mut geometry: Option<RawGeometry> = None;
    let mut document_label = String::new();

    loop {
        events = events.saturating_add(1);
        if events > MAX_XML_EVENTS {
            return Err(IoError::Invalid(format!("{file}: Document.xml has too many elements")));
        }
        let event = reader.read_event_into(&mut buf).map_err(|e| IoError::Invalid(format!("{file}: malformed Document.xml: {e}")))?;
        match event {
            Event::Start(ref e) => {
                depth = depth.saturating_add(1);
                if depth > MAX_XML_DEPTH {
                    return Err(IoError::Invalid(format!("{file}: Document.xml is nested too deeply")));
                }
                match e.local_name().as_ref() {
                    b"Objects" => in_objects = true,
                    b"ObjectData" => in_object_data = true,
                    b"Object" if in_object_data => {
                        if objects.len() >= MAX_OBJECTS {
                            return Err(IoError::Invalid(format!("{file}: Document.xml has too many objects")));
                        }
                        let a = attrs(e);
                        let source_name = string(&a, "name");
                        current = Some(RawObject {
                            type_name: types.get(&source_name).cloned().unwrap_or_default(),
                            label: source_name.clone(),
                            source_name,
                            ..RawObject::default()
                        });
                    }
                    b"Property" => property = string(&attrs(e), "name"),
                    b"Geometry" if property == "Geometry" => {
                        geometry = Some(RawGeometry { type_name: string(&attrs(e), "type"), construction: false, data: GeometryData::Unsupported });
                    }
                    _ => apply_empty(e, &property, current.as_mut(), geometry.as_mut(), &mut document_label),
                }
            }
            Event::Empty(ref e) => {
                if in_objects && !in_object_data && e.local_name().as_ref() == b"Object" {
                    if types.len() >= MAX_OBJECTS {
                        return Err(IoError::Invalid(format!("{file}: Document.xml has too many objects")));
                    }
                    let a = attrs(e);
                    let name = string(&a, "name");
                    if !name.is_empty() {
                        types.insert(name, string(&a, "type"));
                    }
                } else {
                    apply_empty(e, &property, current.as_mut(), geometry.as_mut(), &mut document_label);
                }
            }
            Event::End(ref e) => {
                match e.local_name().as_ref() {
                    b"Objects" => in_objects = false,
                    b"ObjectData" => in_object_data = false,
                    b"Property" => property.clear(),
                    b"Geometry" => {
                        if let (Some(object), Some(raw)) = (current.as_mut(), geometry.take()) {
                            object.geometry.push(raw);
                        }
                    }
                    b"Object" if in_object_data => {
                        if let Some(object) = current.take() {
                            objects.push(object);
                        }
                    }
                    _ => {}
                }
                depth = depth.saturating_sub(1);
            }
            Event::Eof if depth == 0 => break,
            Event::Eof => return Err(IoError::Invalid(format!("{file}: malformed Document.xml: unclosed element"))),
            _ => {}
        }
        buf.clear();
    }
    project(document_label, objects, file)
}

fn apply_empty(e: &BytesStart<'_>, property: &str, current: Option<&mut RawObject>, geometry: Option<&mut RawGeometry>, document_label: &mut String) {
    let tag = e.local_name();
    let a = attrs(e);
    let mut current = current;
    if let Some(object) = current.as_deref_mut() {
        operations::capture(object, tag.as_ref(), property, &a);
    }
    match tag.as_ref() {
        b"PropertyPlacement" if property == "Placement" => {
            if let Some(object) = current {
                object.placement = Some(read_placement(&a));
            }
        }
        b"Integer" if property == "MapMode" => {
            if let Some(object) = current {
                object.map_mode = integer(&a, "value", -1);
            }
        }
        b"Link" if matches!(property, "Support" | "AttachmentSupport") => {
            if let Some(object) = current {
                object.support.push((string(&a, "obj"), string(&a, "sub")));
            }
        }
        b"LinkSub" if matches!(property, "Support" | "AttachmentSupport") => {
            if let Some(object) = current {
                let name = string(&a, "value");
                if !name.is_empty() {
                    object.support.push((name, String::new()));
                }
            }
        }
        b"Sub" if matches!(property, "Support" | "AttachmentSupport") => {
            if let Some(object) = current
                && let Some((_, sub)) = object.support.last_mut()
            {
                *sub = string(&a, "value");
            }
        }
        b"String" if property == "Label" => {
            let label = string(&a, "value");
            if let Some(object) = current {
                object.label = label;
            } else if document_label.is_empty() {
                *document_label = label;
            }
        }
        b"Link" if matches!(property, "Group" | "OriginFeatures") => {
            if let Some(object) = current {
                let child = string(&a, "value");
                if !child.is_empty() {
                    object.children.push(child);
                }
            }
        }
        b"GeomPoint" => {
            if let Some(g) = geometry {
                g.data = GeometryData::Point { position: Vec2::new(number(&a, "X"), number(&a, "Y")) };
            }
        }
        b"LineSegment" => {
            if let Some(g) = geometry {
                g.data = GeometryData::Line {
                    start: Vec2::new(number(&a, "StartX"), number(&a, "StartY")),
                    end: Vec2::new(number(&a, "EndX"), number(&a, "EndY")),
                };
            }
        }
        b"Circle" => {
            if let Some(g) = geometry {
                g.data = GeometryData::Circle { center: Vec2::new(number(&a, "CenterX"), number(&a, "CenterY")), radius: number(&a, "Radius") };
            }
        }
        b"ArcOfCircle" => {
            if let Some(g) = geometry {
                g.data = GeometryData::Arc {
                    center: Vec2::new(number(&a, "CenterX"), number(&a, "CenterY")),
                    radius: number(&a, "Radius"),
                    start: number(&a, "StartAngle"),
                    end: number(&a, "EndAngle"),
                    angle: number(&a, "AngleXU"),
                };
            }
        }
        b"Ellipse" => {
            if let Some(g) = geometry {
                g.data = GeometryData::Ellipse {
                    center: Vec2::new(number(&a, "CenterX"), number(&a, "CenterY")),
                    major: number(&a, "MajorRadius"),
                    minor: number(&a, "MinorRadius"),
                    angle: number(&a, "AngleXU"),
                };
            }
        }
        b"Construction" => {
            if let Some(g) = geometry {
                g.construction = string(&a, "value") == "1";
            }
        }
        b"Constrain" if property == "Constraints" => {
            if let Some(object) = current {
                object.constraints.push(RawConstraint {
                    kind: integer(&a, "Type", -1),
                    value: number(&a, "Value"),
                    first: integer(&a, "First", -2000),
                    first_pos: integer(&a, "FirstPos", 0),
                    second: integer(&a, "Second", -2000),
                    second_pos: integer(&a, "SecondPos", 0),
                    third: integer(&a, "Third", -2000),
                    third_pos: integer(&a, "ThirdPos", 0),
                    driving: integer(&a, "IsDriving", 1) != 0,
                    active: integer(&a, "IsActive", 1) != 0,
                    virtual_space: integer(&a, "IsInVirtualSpace", 0) != 0,
                });
            }
        }
        _ => {}
    }
}

fn object_kind(type_name: &str) -> Option<FreeCadNodeKind> {
    match type_name {
        "App::Part" => Some(FreeCadNodeKind::Part),
        "PartDesign::Body" => Some(FreeCadNodeKind::Body),
        "Sketcher::SketchObject" | "Sketcher::SketchObjectPython" => Some(FreeCadNodeKind::Sketch),
        _ => None,
    }
}

fn read_placement(a: &BTreeMap<String, String>) -> Option<Plane> {
    let value = |key: &str| a.get(key)?.parse::<f64>().ok().filter(|v| v.is_finite());
    let origin = Vec3::new(value("Px")?, value("Py")?, value("Pz")?);
    if origin.x.abs().max(origin.y.abs()).max(origin.z.abs()) > 1e9 {
        return None;
    }
    // FreeCAD saves quaternion components as x, y, z, w (Q0 through Q3).
    let (x, y, z, w) = (value("Q0")?, value("Q1")?, value("Q2")?, value("Q3")?);
    let norm = x * x + y * y + z * z + w * w;
    if !norm.is_finite() || norm < 1e-20 {
        return None;
    }
    let scale = norm.sqrt();
    let (x, y, z, w) = (x / scale, y / scale, z / scale, w / scale);
    Plane::new(
        origin,
        Vec3::new(1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y)),
        Vec3::new(2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)),
    )
}

fn compose_frame(parent: Plane, local: Plane) -> Option<Plane> {
    let rotate = |v: Vec3| parent.x * v.x + parent.y * v.y + parent.normal() * v.z;
    Plane::new(parent.origin + rotate(local.origin), rotate(local.x), rotate(local.y))
}

fn global_frame(name: &str, objects: &HashMap<&str, &RawObject>, parents: &HashMap<&str, &str>) -> Option<Plane> {
    let mut chain = Vec::new();
    let mut current = name;
    loop {
        if chain.len() >= 64 || chain.contains(&current) {
            return None;
        }
        chain.push(current);
        match parents.get(current) {
            Some(parent) => current = parent,
            None => break,
        }
    }
    let mut frame = Plane::XY;
    for name in chain.into_iter().rev() {
        let object = objects.get(name)?;
        frame = compose_frame(frame, object.placement.unwrap_or(Some(Plane::XY))?)?;
    }
    Some(frame)
}

fn datum_dependency_exception(name: &str, objects: &HashMap<&str, &RawObject>, depth: usize) -> Option<String> {
    if depth >= 32 {
        return Some("The datum support chain is cyclic or too deep.".into());
    }
    let object = objects.get(name)?;
    if object.map_mode == 0 {
        return None;
    }
    for (support, sub) in &object.support {
        let Some(target) = objects.get(support.as_str()) else {
            return Some(format!("The datum refers to missing support `{support}`."));
        };
        if !sub.is_empty() || !matches!(target.type_name.as_str(), "App::Plane" | "PartDesign::Plane") {
            return Some(format!("The datum depends on `{support}` ({}) subelement `{sub}`.", target.type_name));
        }
        if target.type_name == "PartDesign::Plane"
            && let Some(reason) = datum_dependency_exception(support, objects, depth + 1)
        {
            return Some(reason);
        }
    }
    None
}

fn recover_plane(object: &RawObject, objects: &HashMap<&str, &RawObject>, parents: &HashMap<&str, &str>, sketch: &mut FreeCadSketch) {
    sketch.saved_plane = global_frame(&object.source_name, objects, parents);
    let fallback = |sketch: &mut FreeCadSketch, reason: String| {
        sketch.attachment_unresolved = true;
        sketch.plane_description = "XY (attachment not preserved)".into();
        warn(
            &mut sketch.warnings,
            "The sketch attachment could not be preserved; the sketch will use XY.",
            format!("{reason} Local sketch geometry and constraints are retained. Use Redefine Sketch Plane after import to choose a new support."),
        );
    };
    if object.map_mode != 0 && object.support.is_empty() {
        // A saved sketch may retain an active MapMode without a recorded support.
        // Its resolved Placement is still a complete, independent sketch plane.
        sketch.plane_description = "Saved sketch plane (source support not recorded)".into();
    } else if object.map_mode != 0 {
        if object.support.len() != 1 {
            fallback(sketch, "The active attachment does not reference exactly one plane.".into());
            return;
        }
        let Some((support_name, sub)) = object.support.first() else { return };
        let Some(support) = objects.get(support_name.as_str()) else {
            fallback(sketch, format!("The support object `{support_name}` is missing from Document.xml."));
            return;
        };
        // Newer saved documents can address an origin plane through an Origin, with
        // the plane's object name in the subelement slot. The Origin's feature list
        // may differ from that name, but the named plane and saved sketch Placement
        // are sufficient for the static plane imported into SolidCraft.
        let origin_plane = if support.type_name == "App::Origin" {
            objects
                .get(sub.as_str())
                .or_else(|| sub.strip_suffix('.').and_then(|name| objects.get(name)))
                .filter(|plane| plane.type_name == "App::Plane")
        } else {
            None
        };
        let (support_name, sub, support) = origin_plane.map(|plane| (&plane.source_name, "", plane)).unwrap_or((support_name, sub.as_str(), support));
        if !sub.is_empty() || !matches!(support.type_name.as_str(), "App::Plane" | "PartDesign::Plane") {
            if support.type_name == "App::Origin"
                && matches!(object.placement, Some(Some(_)))
                && let Some(saved) = sketch.saved_plane
            {
                // A stale Origin subelement does not invalidate the sketch's
                // resolved placement. Keep its static plane without claiming a
                // live attachment to a different Origin feature.
                sketch.plane = saved;
                sketch.plane_description = "Saved sketch plane (source Origin link unavailable)".into();
                return;
            }
            if support.type_name != "App::Origin"
                && matches!(object.placement, Some(Some(_)))
                && let Some(saved) = sketch.saved_plane
            {
                // A face or sketch attachment cannot be reconstructed from a
                // source subelement name. Its resolved Placement is sufficient
                // for an independent, editable sketch on the same static plane.
                sketch.plane = saved;
                let source = if sub.is_empty() { support_name.to_string() } else { format!("{support_name} {sub}") };
                sketch.plane_description = format!("Saved sketch plane (independent of {source})");
                let sketch_label = if object.label.is_empty() { &object.source_name } else { &object.label };
                let support_label = if support.label.is_empty() { support_name.as_str() } else { support.label.as_str() };
                let subelement = if sub.is_empty() { String::new() } else { format!(", subelement `{sub}`") };
                warn_with_severity(
                    &mut sketch.warnings,
                    FreeCadWarningSeverity::Warning,
                    "A dynamic FreeCAD attachment became a static sketch plane.",
                    format!(
                        "FreeCAD sketch `{sketch_label}` (`{}`) was dynamically attached to `{support_label}` (`{support_name}`, {}{subelement}). SolidCraft placed it at its saved position and orientation on a static, independent plane. The sketch and its operations can be edited and evaluated, but changes to the supporting geometry in SolidCraft will not move the sketch automatically. Reattach it to a SolidCraft face or plane if that relationship is needed.",
                        object.source_name, support.type_name,
                    ),
                );
                return;
            }
            fallback(
                sketch,
                format!(
                    "The sketch is attached to `{support_name}` ({}) subelement `{sub}`, and its saved sketch plane is unavailable. Redefine the sketch plane after import.",
                    support.type_name
                ),
            );
            return;
        }
        if object.placement.is_none() {
            fallback(sketch, "The attached sketch has no saved resolved Placement.".into());
            return;
        }
        if support.type_name == "PartDesign::Plane" {
            let Some(frame) = global_frame(support_name, objects, parents) else {
                fallback(sketch, format!("Datum plane `{support_name}` has an invalid placement or parent chain."));
                return;
            };
            let label = if support.label.is_empty() { support_name.clone() } else { support.label.clone() };
            sketch.datum = Some((support_name.clone(), label.clone(), frame));
            sketch.plane_description = format!("Datum plane: {label}");
            if let Some(reason) = datum_dependency_exception(support_name, objects, 0) {
                warn(
                    &mut sketch.warnings,
                    "The datum plane will retain its saved position, but its source dependency is removed.",
                    format!(
                        "{reason} The imported construction plane uses the saved position and orientation. It will not follow changes to the original FreeCAD support."
                    ),
                );
            }
        } else {
            sketch.plane_description = format!("Origin plane: {}", support.label);
        }
    } else {
        sketch.plane_description = "Saved sketch plane".into();
    }
    match global_frame(&object.source_name, objects, parents) {
        Some(frame) => {
            sketch.plane = frame;
            if let Some((_, _, datum)) = &sketch.datum {
                let delta = frame.origin - datum.origin;
                let sideways = delta - datum.normal() * delta.dot(datum.normal());
                if sideways.len() > 1e-7 || frame.normal().dot(datum.normal()) < 1.0 - 1e-10 {
                    warn(
                        &mut sketch.warnings,
                        "The sketch plane is preserved, but its complex datum attachment becomes independent.",
                        "The attachment includes a translation within the datum plane or a tilt relative to it. The imported sketch keeps its saved position and orientation, but does not follow edits to the imported datum plane. Normal offsets and rotation within a datum plane retain their attachment.",
                    );
                }
            }
        }
        None => {
            sketch.datum = None;
            fallback(sketch, "The saved sketch or parent placement is invalid or cyclic.".into());
        }
    }
}

fn project(label: String, objects: Vec<RawObject>, file: &str) -> Result<FreeCadProject> {
    let by_name: HashMap<&str, &RawObject> = objects.iter().map(|o| (o.source_name.as_str(), o)).collect();
    let mut parents = HashMap::new();
    for parent in &objects {
        if matches!(object_kind(&parent.type_name), Some(FreeCadNodeKind::Part | FreeCadNodeKind::Body)) || parent.type_name == "App::Origin" {
            for child in &parent.children {
                parents.insert(child.as_str(), parent.source_name.as_str());
            }
        }
        if let Some((origin, _)) = parent.links.get("Origin")
            && !origin.is_empty()
        {
            parents.insert(origin.as_str(), parent.source_name.as_str());
        }
    }
    let mut sketches = Vec::new();
    let mut sketch_by_name = HashMap::new();
    let mut preview_points = 100_000usize;
    let mut preview_count = 0usize;
    for object in &objects {
        if object_kind(&object.type_name) == Some(FreeCadNodeKind::Sketch) {
            let mut sketch = convert_sketch(object);
            recover_plane(object, &by_name, &parents, &mut sketch);
            if !sketch.warnings.is_empty() && preview_count < 200 {
                sketch.preview = preview(object, &mut preview_points);
                preview_count += 1;
            }
            for warning in &mut sketch.warnings {
                if warning.context.is_none() {
                    warning.context = Some(FreeCadIssueContext { sketch: object.source_name.clone(), references: Vec::new() });
                }
            }
            sketch_by_name.insert(object.source_name.clone(), sketches.len());
            sketches.push(sketch);
        }
    }
    let referenced: BTreeSet<&str> = objects
        .iter()
        .filter(|o| matches!(object_kind(&o.type_name), Some(FreeCadNodeKind::Part | FreeCadNodeKind::Body)))
        .flat_map(|o| o.children.iter().map(String::as_str))
        .collect();
    let mut seen = BTreeSet::new();
    let mut nodes = Vec::new();
    for object in &objects {
        if object_kind(&object.type_name).is_some()
            && !referenced.contains(object.source_name.as_str())
            && let Some(node) = build_node(&object.source_name, &by_name, &sketch_by_name, &mut sketches, &mut seen, &[], &[], None, 0)
        {
            nodes.push(node);
        }
    }
    // Broken group links must not hide an otherwise importable sketch.
    for object in &objects {
        if object_kind(&object.type_name).is_some()
            && !seen.contains(object.source_name.as_str())
            && let Some(node) = build_node(&object.source_name, &by_name, &sketch_by_name, &mut sketches, &mut seen, &[], &[], None, 0)
        {
            nodes.push(node);
        }
    }
    let fallback = std::path::Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "FreeCAD project".into());
    let operations = operations::collect(&objects, &by_name, &parents);
    Ok(FreeCadProject { label: if label.trim().is_empty() { fallback } else { label }, nodes, sketches, operations })
}

fn build_node(
    name: &str,
    objects: &HashMap<&str, &RawObject>,
    sketch_by_name: &HashMap<String, usize>,
    sketches: &mut [FreeCadSketch],
    seen: &mut BTreeSet<String>,
    parent_path: &[String],
    parent_parts: &[(String, String)],
    parent_body: Option<(String, String)>,
    depth: usize,
) -> Option<FreeCadNode> {
    if depth > 32 || !seen.insert(name.to_string()) {
        return None;
    }
    let object = *objects.get(name)?;
    let kind = object_kind(&object.type_name)?;
    let label = if object.label.trim().is_empty() { name.to_string() } else { object.label.trim().chars().take(256).collect() };
    let mut path = parent_path.to_vec();
    path.push(label.clone());
    let mut parts = parent_parts.to_vec();
    if kind == FreeCadNodeKind::Part {
        parts.push((object.source_name.clone(), label.clone()));
    }
    let body = if kind == FreeCadNodeKind::Body { Some((object.source_name.clone(), label.clone())) } else { parent_body };
    let mut children = Vec::new();
    for child in &object.children {
        if let Some(node) = build_node(child, objects, sketch_by_name, sketches, seen, &path, &parts, body.clone(), depth + 1) {
            children.push(node);
        }
    }
    let sketch = sketch_by_name.get(name).and_then(|i| sketches.get_mut(*i)).map(|s| {
        s.path = path;
        s.parts = parts;
        s.body = body;
        FreeCadSketchInfo {
            geometry: s.sketch.curves.len(),
            constraints: s.sketch.constraints.len(),
            plane: s.plane_description.clone(),
            saved_plane_available: s.saved_plane.is_some(),
            attachment_unresolved: s.attachment_unresolved,
            warnings: s.warnings.clone(),
        }
    });
    Some(FreeCadNode { source_name: name.to_string(), label, kind, children, sketch })
}

fn warn(warnings: &mut Vec<FreeCadWarning>, summary: impl Into<String>, detail: impl Into<String>) {
    warn_with_severity(warnings, FreeCadWarningSeverity::Issue, summary, detail);
}

fn warn_with_severity(warnings: &mut Vec<FreeCadWarning>, severity: FreeCadWarningSeverity, summary: impl Into<String>, detail: impl Into<String>) {
    let warning = FreeCadWarning { summary: summary.into(), detail: detail.into(), severity, context: None };
    if warnings.len() < MAX_WARNINGS && !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

fn warn_references(warnings: &mut Vec<FreeCadWarning>, object: &RawObject, references: Vec<(i32, i32)>, summary: String, detail: String) {
    let before = warnings.len();
    warn(warnings, summary, detail);
    if warnings.len() > before
        && let Some(warning) = warnings.last_mut()
    {
        warning.context = Some(FreeCadIssueContext { sketch: object.source_name.clone(), references });
    }
}

fn preview(object: &RawObject, budget: &mut usize) -> FreeCadSketchPreview {
    let mut curves = Vec::new();
    for (geometry, raw) in object.geometry.iter().enumerate().take(256) {
        let needed = if matches!(raw.data, GeometryData::Point { .. }) { 1 } else { 73 };
        if *budget < needed {
            break;
        }
        let (points, start, end, center) = match raw.data {
            GeometryData::Point { position } => (vec![position], Some(position), None, None),
            GeometryData::Line { start, end } => (vec![start, end], Some(start), Some(end), None),
            GeometryData::Circle { center, radius } => {
                let points = (0..=72)
                    .map(|i| {
                        let a = f64::from(i) * std::f64::consts::TAU / 72.0;
                        center + Vec2::new(a.cos(), a.sin()) * radius
                    })
                    .collect();
                (points, None, None, Some(center))
            }
            GeometryData::Arc { center, radius, start, end, angle } => {
                let points: Vec<_> = (0..=72)
                    .map(|i| {
                        let a = angle + start + (end - start) * f64::from(i) / 72.0;
                        center + Vec2::new(a.cos(), a.sin()) * radius
                    })
                    .collect();
                let first = points.first().copied();
                let last = points.last().copied();
                (points, first, last, Some(center))
            }
            GeometryData::Ellipse { center, major, minor, angle } => {
                let points = (0..=72)
                    .map(|i| {
                        let a = f64::from(i) * std::f64::consts::TAU / 72.0;
                        let (x, y) = (major * a.cos(), minor * a.sin());
                        center + Vec2::new(x * angle.cos() - y * angle.sin(), x * angle.sin() + y * angle.cos())
                    })
                    .collect();
                (points, None, None, Some(center))
            }
            GeometryData::Unsupported => continue,
        };
        if points.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.x.abs() <= 1e9 && p.y.abs() <= 1e9) {
            *budget = budget.saturating_sub(points.len());
            curves.push(FreeCadPreviewCurve { geometry, points, construction: raw.construction, start, end, center });
        }
    }
    let omitted = object.geometry.len().saturating_sub(curves.len());
    FreeCadSketchPreview { source: object.source_name.clone(), curves, omitted }
}

fn geometry_name(type_name: &str) -> String {
    match type_name {
        "Part::GeomPoint" => "point".into(),
        "Part::GeomLineSegment" => "line segment".into(),
        "Part::GeomCircle" => "circle".into(),
        "Part::GeomArcOfCircle" => "circular arc".into(),
        "Part::GeomEllipse" => "ellipse".into(),
        "Part::GeomBSplineCurve" => "B-spline curve".into(),
        "Part::GeomBezierCurve" => "Bézier curve".into(),
        "Part::GeomArcOfEllipse" => "elliptical arc".into(),
        "Part::GeomHyperbola" | "Part::GeomArcOfHyperbola" => "hyperbola".into(),
        "Part::GeomParabola" | "Part::GeomArcOfParabola" => "parabola".into(),
        _ => type_name.rsplit("::").next().unwrap_or("unknown geometry").trim_start_matches("Geom").to_string(),
    }
}

fn entity_description(object: &RawObject, entity: i32, position: i32) -> Option<String> {
    if entity == -2000 {
        return None;
    }
    if entity == -1 && position == 1 {
        return Some("the sketch origin".into());
    }
    if entity == -1 {
        return Some("the sketch horizontal (X) axis (FreeCAD reference -1)".into());
    }
    if entity == -2 {
        return Some("the sketch vertical (Y) axis (FreeCAD reference -2)".into());
    }
    if entity < 0 {
        return Some(format!("external geometry reference {entity}"));
    }
    let index = usize::try_from(entity).ok()?;
    let geometry = object.geometry.get(index);
    let kind = geometry.map(|g| geometry_name(&g.type_name)).unwrap_or_else(|| "missing geometry".into());
    let element = match position {
        1 => ", start point",
        2 => ", end point",
        3 => ", center point",
        _ => "",
    };
    Some(format!("geometry {} ({kind}{element}; FreeCAD index {entity})", index + 1))
}

fn constraint_entities(object: &RawObject, constraint: &RawConstraint) -> String {
    [
        entity_description(object, constraint.first, constraint.first_pos),
        entity_description(object, constraint.second, constraint.second_pos),
        entity_description(object, constraint.third, constraint.third_pos),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ")
}

fn convert_sketch(object: &RawObject) -> FreeCadSketch {
    let mut sketch = Sketch::new();
    let mut warnings = Vec::new();
    let mut maps = Vec::with_capacity(object.geometry.len());
    for (index, geometry) in object.geometry.iter().enumerate() {
        let before_points = sketch.points.len();
        if let GeometryData::Point { position } = geometry.data {
            match sketch.add_point(position, None) {
                Ok(point) => maps.push(GeometryMap { start: Some(point), ..GeometryMap::default() }),
                Err(e) => {
                    warn_references(
                        &mut warnings,
                        object,
                        vec![(index as i32, 0)],
                        format!("Geometry {} could not be imported because its values are invalid.", index + 1),
                        format!("FreeCAD geometry {} has type `{}`. SolidCraft rejected it with: {e}", index + 1, geometry.type_name),
                    );
                    maps.push(GeometryMap::default());
                }
            }
            continue;
        }
        let result = match geometry.data {
            GeometryData::Point { .. } => continue,
            GeometryData::Line { start, end } => sketch.add_line(start, end, None, None, None),
            GeometryData::Circle { center, radius } => sketch.add_circle(center, radius, None, None),
            GeometryData::Arc { center, radius, start, end, angle } => {
                let p0 = center + Vec2::new((angle + start).cos(), (angle + start).sin()) * radius;
                let p1 = center + Vec2::new((angle + end).cos(), (angle + end).sin()) * radius;
                sketch.add_arc(center, p0, p1, [None, None, None], None)
            }
            GeometryData::Ellipse { center, major, minor, angle } => {
                let c = sketch.add_point(center, None);
                let m = sketch.add_point(center + Vec2::new(angle.cos(), angle.sin()) * major, None);
                match (c, m) {
                    (Ok(c), Ok(m)) => sketch.add_curve(CurveKind::Ellipse { c, m, r: minor }, None),
                    (Err(e), _) | (_, Err(e)) => Err(e),
                }
            }
            GeometryData::Unsupported => {
                let name = geometry_name(&geometry.type_name);
                warn_references(
                    &mut warnings,
                    object,
                    vec![(index as i32, 0)],
                    format!("Geometry {} is a {name}, which SolidCraft does not import yet.", index + 1),
                    format!(
                        "FreeCAD geometry {} has type `{}`. The importer currently supports points, line segments, circles, circular arcs and ellipses. Constraints that refer to this geometry may also be skipped.",
                        index + 1,
                        geometry.type_name
                    ),
                );
                maps.push(GeometryMap::default());
                continue;
            }
        };
        match result {
            Ok(curve) => {
                if let Some(c) = sketch.curves.get_mut(curve) {
                    c.construction = geometry.construction;
                }
                let points = sketch.curves.get(curve).map(|c| c.kind.point_ids()).unwrap_or_default();
                let map = match sketch.curves.get(curve).map(|c| &c.kind) {
                    Some(CurveKind::Line { a, b }) => GeometryMap { curve: Some(curve), start: Some(*a), end: Some(*b), center: None },
                    Some(CurveKind::Circle { c, .. }) => GeometryMap { curve: Some(curve), center: Some(*c), ..GeometryMap::default() },
                    Some(CurveKind::Arc { c, a, b }) => GeometryMap { curve: Some(curve), start: Some(*a), end: Some(*b), center: Some(*c) },
                    Some(CurveKind::Ellipse { c, .. }) => GeometryMap { curve: Some(curve), center: Some(*c), ..GeometryMap::default() },
                    _ => GeometryMap { curve: Some(curve), start: points.first().copied(), end: points.last().copied(), center: None },
                };
                maps.push(map);
            }
            Err(e) => {
                sketch.points.truncate(before_points);
                warn_references(
                    &mut warnings,
                    object,
                    vec![(index as i32, 0)],
                    format!("Geometry {} could not be imported because its values are invalid.", index + 1),
                    format!("FreeCAD geometry {} has type `{}`. SolidCraft rejected it with: {e}", index + 1, geometry.type_name),
                );
                maps.push(GeometryMap::default());
            }
        }
    }
    for (index, constraint) in object.constraints.iter().enumerate() {
        let references = || {
            [(constraint.first, constraint.first_pos), (constraint.second, constraint.second_pos), (constraint.third, constraint.third_pos)]
                .into_iter()
                .filter(|(entity, position)| entity_description(object, *entity, *position).is_some())
                .collect()
        };
        if !constraint.active || constraint.virtual_space {
            continue;
        }
        if constraint.kind == 17 {
            if let Some(curve) = curve_ref(&maps, constraint.first)
                && let Some(c) = sketch.curves.get_mut(curve)
            {
                c.fixed = true;
            } else {
                let entities = constraint_entities(object, constraint);
                warn_references(
                    &mut warnings,
                    object,
                    references(),
                    format!("Constraint {} fixes geometry that could not be imported.", index + 1),
                    format!("FreeCAD constraint {} is a Block constraint. Referenced entity: {entities}.", index + 1),
                );
            }
            continue;
        }
        let Some(kind) = convert_constraint(constraint, &maps, &mut sketch) else {
            let entities = constraint_entities(object, constraint);
            if constraint_name(constraint.kind).is_some() {
                warn_references(
                    &mut warnings,
                    object,
                    references(),
                    format!("Constraint {} refers to geometry that could not be imported.", index + 1),
                    format!(
                        "FreeCAD constraint {} is {} (type {}). Referenced entities: {entities}. At least one referenced point or curve is unavailable in the imported sketch.",
                        index + 1,
                        constraint_name(constraint.kind).unwrap_or("a supported constraint"),
                        constraint.kind
                    ),
                );
            } else {
                warn_references(
                    &mut warnings,
                    object,
                    references(),
                    format!("Constraint {} uses a FreeCAD constraint type that SolidCraft does not import yet.", index + 1),
                    format!(
                        "FreeCAD constraint {} has numeric type {}. Referenced entities: {entities}. Supported constraints include coincident, horizontal, vertical, parallel, tangent, distance, angle, perpendicular, radius, equal, point-on-object, symmetry, block and diameter.",
                        index + 1,
                        constraint.kind
                    ),
                );
            }
            continue;
        };
        match sketch.add_constraint(kind, None) {
            Ok(_) => {
                if !constraint.driving
                    && let Some(last) = sketch.constraints.last_mut()
                {
                    last.driven = true;
                }
            }
            Err(e) => {
                let entities = constraint_entities(object, constraint);
                warn_references(
                    &mut warnings,
                    object,
                    references(),
                    format!("Constraint {} could not be represented in this sketch.", index + 1),
                    format!(
                        "FreeCAD constraint {} is {} (type {}). Referenced entities: {entities}. SolidCraft rejected it with: {e}",
                        index + 1,
                        constraint_name(constraint.kind).unwrap_or("an unknown constraint"),
                        constraint.kind
                    ),
                );
            }
        }
    }
    FreeCadSketch {
        source_name: object.source_name.clone(),
        label: if object.label.trim().is_empty() { object.source_name.clone() } else { object.label.trim().chars().take(256).collect() },
        path: Vec::new(),
        parts: Vec::new(),
        body: None,
        plane: Plane::XY,
        datum: None,
        plane_description: "XY".into(),
        saved_plane: None,
        attachment_unresolved: false,
        source_curves: maps.iter().map(|map| map.curve.and_then(|index| sketch.curves.get(index)).map(|curve| curve.id.clone())).collect(),
        preview: FreeCadSketchPreview { source: object.source_name.clone(), curves: Vec::new(), omitted: object.geometry.len() },
        sketch,
        warnings,
    }
}

fn constraint_name(kind: i32) -> Option<&'static str> {
    Some(match kind {
        1 => "Coincident",
        2 => "Horizontal",
        3 => "Vertical",
        4 => "Parallel",
        5 => "Tangent",
        6 => "Distance or Length",
        7 => "Horizontal Distance",
        8 => "Vertical Distance",
        9 => "Angle",
        10 => "Perpendicular",
        11 => "Radius",
        12 => "Equal",
        13 => "Point on Object",
        14 => "Symmetry",
        17 => "Block",
        18 => "Diameter",
        _ => return None,
    })
}

fn curve_ref(maps: &[GeometryMap], entity: i32) -> Option<usize> {
    usize::try_from(entity).ok().and_then(|i| maps.get(i)).and_then(|m| m.curve)
}

fn point_ref(maps: &[GeometryMap], entity: i32, position: i32) -> Option<usize> {
    if entity == -1 && position == 1 {
        return Some(0);
    }
    let map = usize::try_from(entity).ok().and_then(|i| maps.get(i))?;
    match position {
        1 => map.start,
        2 => map.end,
        3 => map.center,
        _ => None,
    }
}

fn line_points(maps: &[GeometryMap], entity: i32) -> Option<(usize, usize)> {
    let map = usize::try_from(entity).ok().and_then(|i| maps.get(i))?;
    Some((map.start?, map.end?))
}

fn convert_constraint(c: &RawConstraint, maps: &[GeometryMap], sketch: &mut Sketch) -> Option<ConstraintKind> {
    let first_curve = || curve_ref(maps, c.first);
    let second_curve = || curve_ref(maps, c.second);
    let first_point = || point_ref(maps, c.first, c.first_pos);
    let second_point = || point_ref(maps, c.second, c.second_pos);
    Some(match c.kind {
        1 => ConstraintKind::Coincident { p: first_point()?, q: second_point()? },
        // These geometric alignments must remain geometric rather than creating editable zero dimensions.
        2 if c.first_pos != 0 && c.second_pos != 0 => ConstraintKind::HorizontalPoints { p: first_point()?, q: second_point()? },
        2 => ConstraintKind::Horizontal { l: first_curve()? },
        3 if c.first_pos != 0 && c.second_pos != 0 => ConstraintKind::VerticalPoints { p: first_point()?, q: second_point()? },
        3 => ConstraintKind::Vertical { l: first_curve()? },
        4 => ConstraintKind::Parallel { a: first_curve()?, b: second_curve()? },
        5 => ConstraintKind::Tangent { a: first_curve()?, b: second_curve()? },
        6 if c.first_pos != 0 && c.second_pos != 0 => ConstraintKind::Distance { p: first_point()?, q: second_point()?, value: c.value.abs() },
        6 => ConstraintKind::Length { l: first_curve()?, value: c.value.abs() },
        7 if c.first_pos != 0 && c.second_pos != 0 => ConstraintKind::DistanceX { p: first_point()?, q: second_point()?, value: c.value },
        7 => {
            let (p, q) = line_points(maps, c.first)?;
            ConstraintKind::DistanceX { p, q, value: c.value }
        }
        8 if c.first_pos != 0 && c.second_pos != 0 => ConstraintKind::DistanceY { p: first_point()?, q: second_point()?, value: c.value },
        8 => {
            let (p, q) = line_points(maps, c.first)?;
            ConstraintKind::DistanceY { p, q, value: c.value }
        }
        9 if c.second >= 0 => ConstraintKind::Angle { a: first_curve()?, b: second_curve()?, value: c.value, flip: false },
        9 => ConstraintKind::LineAngle { l: first_curve()?, value: c.value },
        10 => ConstraintKind::Perpendicular { a: first_curve()?, b: second_curve()? },
        11 => ConstraintKind::Radius { c: first_curve()?, value: c.value.abs() },
        12 => ConstraintKind::Equal { a: first_curve()?, b: second_curve()? },
        // FreeCAD reserves -1/-2 for the sketch's own infinite axes; projected external geometry
        // starts at -3. Aligning with the fixed origin preserves point-on-axis without helper lines.
        13 if c.second == -1 && c.second_pos == 0 => ConstraintKind::HorizontalPoints { p: 0, q: first_point()? },
        13 if c.second == -2 && c.second_pos == 0 => ConstraintKind::VerticalPoints { p: 0, q: first_point()? },
        13 => ConstraintKind::PointOnCurve { p: first_point()?, c: second_curve()? },
        14 if c.third_pos != 0 => {
            let (p, q, center) = (first_point()?, second_point()?, point_ref(maps, c.third, c.third_pos)?);
            if p == q || p == center || q == center {
                return None;
            }
            // SolidCraft represents symmetry about a point as a construction
            // line between the two points with its midpoint on the center.
            let l = sketch.add_line_pts(p, q, None).ok()?;
            sketch.curves.get_mut(l)?.construction = true;
            ConstraintKind::Midpoint { p: center, l }
        }
        14 => {
            let (p, q) = (first_point()?, second_point()?);
            let l = match (c.third, c.third_pos) {
                (-1, 0) => sketch.sketch_axis_line("x").ok().flatten()?,
                (-2, 0) => sketch.sketch_axis_line("y").ok().flatten()?,
                _ => curve_ref(maps, c.third)?,
            };
            ConstraintKind::Symmetric { p, q, l }
        }
        18 => ConstraintKind::Diameter { c: first_curve()?, value: c.value.abs() },
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
