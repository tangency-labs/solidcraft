//! Lengths at the UI boundary. Bare values use the current design unit; explicit units and
//! parameter expressions keep their meaning. The engine and saved geometry remain in mm.

use solidcraft_engine::doc::{
    Document,
    expr::{Kind, unit_info},
};

pub(crate) fn unit(doc: &Document) -> &str {
    solidcraft_engine::doc::expr::length_unit_or_mm(&doc.units)
}

/// A new field follows the same numeric-default policy as the primitive and sketch dialogs.
pub(crate) fn default(doc: &Document, number: &str) -> String {
    format!("{} {}", number.trim(), unit(doc))
}

/// Apply the design unit to numeric defaults from declarative dialog descriptors.
/// Called only for new dialogs; stored expressions keep their original units.
pub(crate) fn defaults(doc: &Document, mut kind: crate::dialogs::Kind) -> crate::dialogs::Kind {
    for value in fields(doc, &mut kind) {
        if let Some(number) = value.strip_suffix(" mm")
            && solidcraft_engine::doc::expr::is_literal(number)
        {
            *value = default(doc, number);
        }
    }
    kind
}

/// Freeze the unit of UI input before handing it to an API that reads bare lengths in mm.
/// Keep named and dimensional expressions intact so they remain driving expressions.
pub(crate) fn expression(doc: &Document, text: &str, kind: Kind) -> String {
    let text = text.trim();
    let normalized = doc.with_design_unit(text, kind);
    if kind == Kind::Length && unit(doc) != "mm" && normalized == text && doc.eval(text, Kind::Unitless).is_ok() {
        // A decimal comma is numeric only outside parentheses. Preserve function-argument
        // commas while moving the expression inside the unit multiplication's parentheses.
        let mut depth = 0usize;
        let grouped: String = text
            .chars()
            .map(|c| match c {
                '(' => {
                    depth = depth.saturating_add(1);
                    c
                }
                ')' => {
                    depth = depth.saturating_sub(1);
                    c
                }
                ',' if depth == 0 => '.',
                _ => c,
            })
            .collect();
        format!("({grouped}) * 1 {}", unit(doc))
    } else {
        normalized
    }
}

pub(crate) fn eval(doc: &Document, text: &str, kind: Kind) -> solidcraft_engine::doc::Result<f64> {
    doc.eval(&expression(doc, text, kind), kind)
}

/// Legacy bare literals are millimetres, including decimal-comma literals. An unchanged
/// edit must identify that unit before the UI applies its current design-unit default.
pub(crate) fn stored(doc: &Document, text: &str) -> String {
    if solidcraft_engine::doc::expr::is_literal(text) && doc.eval(text, Kind::Unitless).is_ok() {
        format!("{} mm", text.trim())
    } else {
        text.to_string()
    }
}

/// A computed model-space value written back to an editable field. Use all significant digits:
/// display precision must never round a value that will be sent back to the model.
pub(crate) fn model(doc: &Document, mm: f64) -> String {
    let unit = unit(doc);
    let scale = unit_info(unit).map(|(_, scale)| scale).unwrap_or(1.0);
    format!("{} {unit}", mm / scale)
}

/// The diameter in a radius worked out from one, `(D) / 2`: what a diameter typed into Cylinder
/// or Sphere is kept as (#6609). `None` for any other radius, such as `(a) / (b) / 2`.
pub(crate) fn diameter_of(radius: &str) -> Option<&str> {
    inside(radius, "(", ") / 2")
}

/// A diameter switched to the radius (`twice` false) or a radius to the diameter, so the size
/// stays: a number keeps its unit (`20 mm` and `10 mm`), a size switched before goes back to its
/// text, and any other expression is wrapped (`(d) / 2`, `2 * (r)`).
pub(crate) fn scaled(text: &str, twice: bool) -> String {
    let text = text.trim();
    let back = if twice { diameter_of(text) } else { inside(text, "2 * (", ")") };
    if let Some(back) = back {
        return back.to_string();
    }
    if text.is_empty() {
        return String::new();
    }
    if solidcraft_engine::doc::expr::is_literal(text) {
        // The unit is the word at the end (`1e3 mm` is a thousand millimetres).
        let number = text.trim_end_matches(char::is_alphabetic);
        let unit = text.get(number.len()..).unwrap_or_default();
        if let Ok(x) = number.replace(char::is_whitespace, "").replace(',', ".").parse::<f64>() {
            let x = if twice { x * 2.0 } else { x / 2.0 };
            return if unit.is_empty() { format!("{x}") } else { format!("{x} {unit}") };
        }
    }
    if twice { format!("2 * ({text})") } else { format!("({text}) / 2") }
}

/// The text between `open` and `close` when they wrap all of `text`: the parenthesis `open` ends
/// with is the one `close` starts with.
fn inside<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let inner = text.trim().strip_prefix(open)?.strip_suffix(close)?;
    let mut depth = 0usize;
    for c in inner.chars() {
        match c {
            '(' => depth = depth.saturating_add(1),
            ')' => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    (depth == 0 && !inner.trim().is_empty()).then_some(inner.trim())
}

/// The editable length expressions in a dialog. Counts, angles, ratios, selectors and standard
/// part designations never go through length conversion. Keep this list shared by submission
/// and feature refill, so a bare stored millimetre cannot become inches just by pressing OK.
pub(crate) fn fields<'a>(doc: &Document, kind: &'a mut crate::dialogs::Kind) -> Vec<&'a mut String> {
    use crate::dialogs::Kind as D;
    use crate::dialogs_assembly::Asm;
    use crate::dialogs_plastic::Pl;
    use crate::dialogs_sheet::Sm;
    use crate::dialogs_surface::Surf;
    match kind {
        D::Extrude { distance, distance2, to_offset, start, wall, wall2, .. } => vec![distance, distance2, to_offset, start, wall, wall2],
        D::Fillet { radius, distance2, .. } => vec![radius, distance2],
        D::Shell { thickness } => vec![thickness],
        D::PatternRect { spacing, spacing2, .. } => vec![spacing, spacing2],
        D::OffsetPlane { offset } | D::Section { offset, .. } => vec![offset],
        D::OffsetFaces { distance } => vec![distance],
        D::MeshConvert { tolerance } => vec![tolerance],
        D::Thread { length, .. } | D::Emboss { depth: length, .. } => vec![length],
        D::Rib { thickness, depth, .. } => vec![thickness, depth],
        D::PathPattern { spacing, .. } => vec![spacing],
        D::Pipe { diameter, wall, .. } => vec![diameter, wall],
        D::Stock { margin } => vec![margin],
        D::Move { x, y, z, .. } => vec![x, y, z],
        D::Hole { diameter, depth, cb_diameter, cb_depth, cs_diameter, .. } => vec![diameter, depth, cb_diameter, cb_depth, cs_diameter],
        D::Primitive { fields, .. } => fields.iter_mut().filter(|(key, _)| *key != "revolutions").map(|(_, value)| value).collect(),
        D::Surface(k) => match k {
            Surf::Extrude { distance, .. } | Surf::Offset { distance, .. } | Surf::Ruled { distance, .. } | Surf::Extend { distance } => {
                vec![distance]
            }
            Surf::Thicken { thickness, .. } => vec![thickness],
            Surf::Stitch { tolerance } => vec![tolerance],
            _ => vec![],
        },
        D::Sheet(k) => match k {
            Sm::Flange { height, radius, distance, relief_size, own, .. } => {
                let mut values = vec![height, radius, distance, relief_size];
                values.extend(own.iter_mut().map(|(_, _, height)| height));
                values
            }
            Sm::Hem { length, gap, .. } => vec![length, gap],
            Sm::Fold { radius, .. } => vec![radius],
            // K factor is the only dimensionless sheet rule value.
            Sm::Rules { values, .. } => values.iter_mut().enumerate().filter(|(i, _)| *i != 1).map(|(_, value)| value).collect(),
            _ => vec![],
        },
        D::Plastic(k) => match k {
            Pl::Boss { diameter, height, hole_diameter, hole_depth, fillet, rib_thickness, rib_length, rib_offset, .. } => {
                vec![diameter, height, hole_diameter, hole_depth, fillet, rib_thickness, rib_length, rib_offset]
            }
            Pl::Lip { width, height, gap, .. } => vec![width, height, gap],
            Pl::SnapFit { length, thickness, width, catch_depth, catch_length, base_height, .. } => {
                vec![length, thickness, width, catch_depth, catch_length, base_height]
            }
            Pl::Rest { width, length, height, thickness, .. } => vec![width, length, height, thickness],
            Pl::Rules { values, .. } => {
                let kinds = solidcraft_engine::doc::plastic::PlasticRule::library()
                    .first()
                    .map(|rule| rule.fields().iter().map(|(_, _, kind)| *kind).collect::<Vec<_>>())
                    .unwrap_or_default();
                values.iter_mut().skip(1).zip(kinds).filter_map(|(value, kind)| (kind == Kind::Length).then_some(value)).collect()
            }
            _ => vec![],
        },
        D::Assembly(k) => match k {
            Asm::Joint(f) => {
                let kind = f.joint_kind();
                let mut values: Vec<_> = f.offset.iter_mut().collect();
                for (i, (_, lo, hi)) in f.limits.iter_mut().enumerate() {
                    if !kind.is_angle(i) {
                        values.extend([lo, hi]);
                    }
                }
                values
            }
            Asm::Drive { joint, values, .. } => values
                .iter_mut()
                .enumerate()
                .filter(|(i, _)| doc.assembly.joints.get(*joint).is_some_and(|j| !j.kind.is_angle(*i)))
                .map(|(_, v)| v)
                .collect(),
            Asm::MotionLink { b, ib, offset, .. }
                if doc
                    .assembly
                    .joints
                    .iter()
                    .filter(|j| j.kind != solidcraft_engine::doc::joints::JointKind::Rigid)
                    .nth(*b)
                    .is_some_and(|j| !j.kind.is_angle(*ib)) =>
            {
                vec![offset]
            }
            Asm::Coupling(f) => f.sizes.iter_mut().collect(),
            Asm::Cam(f) => vec![&mut f.radius],
            Asm::Interference { clearance, .. } => vec![clearance],
            Asm::Trace(f) if doc.assembly.joints.get(f.joint).is_some_and(|j| !j.kind.is_angle(f.index)) => vec![&mut f.from, &mut f.to],
            _ => vec![],
        },
        D::Motion(crate::dialogs_motion::Mo::MotionStudy { tracks, .. }) => tracks
            .iter_mut()
            .filter(|t| doc.assembly.joints.get(t.joint).is_some_and(|j| !j.kind.is_angle(t.index)))
            .flat_map(|t| t.points.iter_mut().map(|(_, v)| v))
            .collect(),
        D::SketchModify(k) => {
            use crate::sketch_modify_ui::Mod;
            match k {
                Mod::Offset { distance, distance2, .. } => vec![distance, distance2],
                Mod::Move { x, y, .. } => vec![x, y],
                Mod::PatternRect { spacing, spacing2, .. } => vec![spacing, spacing2],
                Mod::Corner { size, distance2, .. } => vec![size, distance2],
                _ => vec![],
            }
        }
        _ => vec![],
    }
}

/// Extra length expressions of variable fillets and surface extents. Geometric picks and
/// direction vectors are already in model coordinates and must not be converted.
pub(crate) fn extra(extra: &mut serde_json::Map<String, serde_json::Value>, mut convert: impl FnMut(&str) -> String) {
    use serde_json::{Value, json};
    for key in ["radius2", "start_offset", "to_offset", "distance2", "thickness", "thickness2"] {
        if let Some(value) = extra.get_mut(key) {
            let text = value.as_str().map(str::to_string).or_else(|| value.as_number().map(ToString::to_string));
            if let Some(text) = text {
                *value = json!(convert(&text));
            }
        }
    }
    if let Some(Value::Array(radii)) = extra.get_mut("radii") {
        for value in radii {
            if let Some(text) = value.as_str() {
                *value = json!(convert(text));
            }
        }
    }
    if let Some(Value::Array(mid)) = extra.get_mut("mid") {
        for point in mid {
            if let Some(value) = point.as_array_mut().and_then(|p| p.get_mut(1))
                && let Some(text) = value.as_str()
            {
                *value = json!(convert(text));
            }
        }
    }
}
