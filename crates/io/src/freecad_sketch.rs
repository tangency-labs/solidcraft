//! Editable sketches from FreeCAD `.FCStd` archives.
//!
//! This is a clean-room reader of the public ZIP/XML interchange representation. It does not
//! link to FreeCAD or use its source. Sketch attachment/support links deliberately stay behind:
//! imported sketches keep their local 2D coordinates on a SolidCraft origin plane.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec2;
use solidcraft_sketch::{ConstraintKind, CurveKind, Sketch};

use crate::{IoError, Result};

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// One converted sketch and its source identity.
#[derive(Clone, Debug)]
pub struct FreeCadSketch {
    pub source_name: String,
    pub label: String,
    pub path: Vec<String>,
    pub sketch: Sketch,
    pub warnings: Vec<String>,
}

/// The selectable hierarchy and converted sketches in an FCStd file.
#[derive(Clone, Debug)]
pub struct FreeCadProject {
    pub label: String,
    pub nodes: Vec<FreeCadNode>,
    pub sketches: Vec<FreeCadSketch>,
}

#[derive(Clone, Debug, Default)]
struct RawObject {
    source_name: String,
    type_name: String,
    label: String,
    children: Vec<String>,
    geometry: Vec<RawGeometry>,
    constraints: Vec<RawConstraint>,
}

#[derive(Clone, Debug)]
struct RawGeometry {
    type_name: String,
    construction: bool,
    data: GeometryData,
}

#[derive(Clone, Debug, Default)]
enum GeometryData {
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
    match tag.as_ref() {
        b"String" if property == "Label" => {
            let label = string(&a, "value");
            if let Some(object) = current {
                object.label = label;
            } else if document_label.is_empty() {
                *document_label = label;
            }
        }
        b"Link" if property == "Group" => {
            if let Some(object) = current {
                let child = string(&a, "value");
                if !child.is_empty() {
                    object.children.push(child);
                }
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

fn project(label: String, objects: Vec<RawObject>, file: &str) -> Result<FreeCadProject> {
    let mut sketches = Vec::new();
    let mut sketch_by_name = HashMap::new();
    for object in &objects {
        if object_kind(&object.type_name) == Some(FreeCadNodeKind::Sketch) {
            let sketch = convert_sketch(object);
            sketch_by_name.insert(object.source_name.clone(), sketches.len());
            sketches.push(sketch);
        }
    }
    let by_name: HashMap<&str, &RawObject> = objects.iter().map(|o| (o.source_name.as_str(), o)).collect();
    let referenced: BTreeSet<&str> = objects
        .iter()
        .filter(|o| matches!(object_kind(&o.type_name), Some(FreeCadNodeKind::Part | FreeCadNodeKind::Body)))
        .flat_map(|o| o.children.iter().map(String::as_str))
        .collect();
    let mut seen = BTreeSet::new();
    let mut nodes = Vec::new();
    for object in &objects {
        if object_kind(&object.type_name).is_some() && !referenced.contains(object.source_name.as_str()) {
            if let Some(node) = build_node(&object.source_name, &by_name, &sketch_by_name, &mut sketches, &mut seen, &[], 0) {
                nodes.push(node);
            }
        }
    }
    // Broken group links must not hide an otherwise importable sketch.
    for object in &objects {
        if object_kind(&object.type_name).is_some()
            && !seen.contains(object.source_name.as_str())
            && let Some(node) = build_node(&object.source_name, &by_name, &sketch_by_name, &mut sketches, &mut seen, &[], 0)
        {
            nodes.push(node);
        }
    }
    let fallback = std::path::Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "FreeCAD project".into());
    Ok(FreeCadProject { label: if label.trim().is_empty() { fallback } else { label }, nodes, sketches })
}

fn build_node(
    name: &str,
    objects: &HashMap<&str, &RawObject>,
    sketch_by_name: &HashMap<String, usize>,
    sketches: &mut [FreeCadSketch],
    seen: &mut BTreeSet<String>,
    parent_path: &[String],
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
    let mut children = Vec::new();
    for child in &object.children {
        if let Some(node) = build_node(child, objects, sketch_by_name, sketches, seen, &path, depth + 1) {
            children.push(node);
        }
    }
    let sketch = sketch_by_name.get(name).and_then(|i| sketches.get_mut(*i)).map(|s| {
        s.path = path;
        FreeCadSketchInfo { geometry: s.sketch.curves.len(), constraints: s.sketch.constraints.len(), warnings: s.warnings.clone() }
    });
    Some(FreeCadNode { source_name: name.to_string(), label, kind, children, sketch })
}

fn warn(warnings: &mut Vec<String>, message: impl Into<String>) {
    let message = message.into();
    if warnings.len() < MAX_WARNINGS && !warnings.contains(&message) {
        warnings.push(message);
    }
}

fn convert_sketch(object: &RawObject) -> FreeCadSketch {
    let mut sketch = Sketch::new();
    let mut warnings = Vec::new();
    let mut maps = Vec::with_capacity(object.geometry.len());
    for (index, geometry) in object.geometry.iter().enumerate() {
        let before_points = sketch.points.len();
        let result = match geometry.data {
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
                warn(&mut warnings, format!("geometry {} ({}) is not supported", index + 1, geometry.type_name));
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
                warn(&mut warnings, format!("geometry {} ({}) was skipped: {e}", index + 1, geometry.type_name));
                maps.push(GeometryMap::default());
            }
        }
    }
    for (index, constraint) in object.constraints.iter().enumerate() {
        if !constraint.active || constraint.virtual_space {
            continue;
        }
        if constraint.kind == 17 {
            if let Some(curve) = curve_ref(&maps, constraint.first)
                && let Some(c) = sketch.curves.get_mut(curve)
            {
                c.fixed = true;
            } else {
                warn(&mut warnings, format!("constraint {} (block) refers to skipped geometry", index + 1));
            }
            continue;
        }
        let Some(kind) = convert_constraint(constraint, &maps) else {
            warn(
                &mut warnings,
                format!("constraint {} (FreeCAD type {}) is not supported or refers to skipped geometry", index + 1, constraint.kind),
            );
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
            Err(e) => warn(&mut warnings, format!("constraint {} was skipped: {e}", index + 1)),
        }
    }
    FreeCadSketch {
        source_name: object.source_name.clone(),
        label: if object.label.trim().is_empty() { object.source_name.clone() } else { object.label.trim().chars().take(256).collect() },
        path: Vec::new(),
        sketch,
        warnings,
    }
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

fn convert_constraint(c: &RawConstraint, maps: &[GeometryMap]) -> Option<ConstraintKind> {
    let first_curve = || curve_ref(maps, c.first);
    let second_curve = || curve_ref(maps, c.second);
    let first_point = || point_ref(maps, c.first, c.first_pos);
    let second_point = || point_ref(maps, c.second, c.second_pos);
    Some(match c.kind {
        1 => ConstraintKind::Coincident { p: first_point()?, q: second_point()? },
        2 => ConstraintKind::Horizontal { l: first_curve()? },
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
        13 => ConstraintKind::PointOnCurve { p: first_point()?, c: second_curve()? },
        14 => ConstraintKind::Symmetric { p: first_point()?, q: second_point()?, l: curve_ref(maps, c.third)? },
        18 => ConstraintKind::Diameter { c: first_curve()?, value: c.value.abs() },
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
