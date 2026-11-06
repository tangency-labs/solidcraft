//! Lengths at the UI boundary. Bare values use the current design unit; explicit units and
//! parameter expressions keep their meaning. The engine and saved geometry remain in mm.

use std::cell::Cell;

use solidcraft_engine::doc::{
    Document,
    expr::{Kind, unit_info},
};

/// How far a size shown in another unit may drift from the size the tool ships with: 2% keeps
/// an inch design's "0.4 in" for 10 mm, close enough to be the same size, and short enough to
/// read in the design's own units.
const CONVERSION_TOLERANCE: f64 = 0.02;

/// The most digits a size written in another unit is given.
const MAX_DIGITS: usize = 6;

thread_local! {
    /// Whether a new length field starts with the size its tool ships with (#12418). Off: it
    /// starts with the number the tool ships with, read in the design's units ("10 in" in an
    /// inch design), the behaviour before #12418. The preference is handed to the thread that
    /// draws the frame, which is the one that builds the dialogs.
    static SCALED_LENGTH_DEFAULTS: Cell<bool> = const { Cell::new(true) };
}

/// Take the preference from the preferences window (#12418).
pub(crate) fn set_scaled_length_defaults(scaled: bool) {
    SCALED_LENGTH_DEFAULTS.with(|cell| cell.set(scaled));
}

/// Whether new length fields keep the size their tool ships with (#12418).
fn scaled_length_defaults() -> bool {
    SCALED_LENGTH_DEFAULTS.with(Cell::get)
}

pub(crate) fn unit(doc: &Document) -> &str {
    solidcraft_engine::doc::expr::length_unit_or_mm(&doc.units)
}

/// A new field follows the same numeric-default policy as the primitive and sketch dialogs.
/// The number is a size in millimetres, the unit the tools ship with, shown in the design's
/// unit: "10 mm" in a millimetre design, "1 cm" in a centimetre one and "0.4 in" in an inch
/// one (#12418). Off, the number is read in the design's unit instead ("10 in").
pub(crate) fn default(doc: &Document, number: &str) -> String {
    let number = number.trim();
    match shipped_size(doc, number) {
        Some(size) => size,
        None => format!("{number} {}", unit(doc)),
    }
}

/// A primitive's size: the number in the design's units ("20 in"), the way the primitive dialogs
/// have always shown it (#630). A primitive is a size the user asks for, not a tool's preset, so
/// its size keeps the design's units and the preference that scales the operation dialogs does
/// not reach it (#12418).
pub(crate) fn primitive_default(doc: &Document, number: &str) -> String {
    format!("{} {}", number.trim(), unit(doc))
}

/// The size a tool ships with, in millimetres, shown in the design's unit (#12418). The size
/// keeps the millimetres the tool ships with, so an extrude of 10 mm is 10 mm in every design,
/// and it is written with as few digits as the design's unit allows, so that an inch design
/// reads "0.4 in" and not "0.39370079 in".
fn shipped_size(doc: &Document, number: &str) -> Option<String> {
    if !scaled_length_defaults() {
        return None;
    }
    let millimetres: f64 = number.parse().ok()?;
    if !millimetres.is_finite() {
        return None;
    }
    let unit = unit(doc);
    let scale = unit_info(unit).map(|(_, scale)| scale)?;
    let value = millimetres / scale;
    // A metric unit converts exactly; an imperial one has no exact short form, so it keeps
    // the size within 2% of the millimetres the tool ships with.
    let text = if matches!(unit, "mm" | "cm" | "m") { exact_number(value) } else { short_number(value) };
    Some(format!("{text} {unit}"))
}

/// The shortest number of digits that writes `value` exactly: "0.15", not "0.15000000000000002".
fn exact_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    for digits in 1..=MAX_DIGITS {
        let rounded = round_significant(value, digits);
        if (rounded - value).abs() <= 1e-9 * value.abs() {
            return format_significant(rounded, digits);
        }
    }
    format_significant(value, MAX_DIGITS)
}

/// The shortest number of digits that keeps `value` within [`CONVERSION_TOLERANCE`] of it:
/// "0.4" for 0.39370079, "0.08" for 0.07874016.
fn short_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    for digits in 1..=MAX_DIGITS {
        let rounded = round_significant(value, digits);
        if (rounded - value).abs() <= CONVERSION_TOLERANCE * value.abs() {
            return format_significant(rounded, digits);
        }
    }
    format_significant(value, MAX_DIGITS)
}

/// `value` rounded to `digits` significant digits. Crate-visible so that a test can ask whether
/// a shorter form would still keep the size (#12418).
pub(crate) fn round_significant(value: f64, digits: usize) -> f64 {
    if value == 0.0 {
        return 0.0;
    }
    let factor = 10f64.powi(digits as i32 - 1 - exponent(value));
    (value * factor).round() / factor
}

/// The power of ten of `value`'s leading digit: 0 for 1, 1 for 20, -1 for 0.4.
fn exponent(value: f64) -> i32 {
    value.abs().log10().floor() as i32
}

/// `value` written with `digits` significant digits, without trailing zeros.
fn format_significant(value: f64, digits: usize) -> String {
    let decimals = (digits as i32 - 1 - exponent(value)).max(0) as usize;
    let text = format!("{value:.decimals$}");
    if decimals == 0 {
        return text;
    }
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Apply the design unit to numeric defaults from declarative dialog descriptors.
/// The number is a size in millimetres, shown in the design's unit (#12418).
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
    // A definition's value is frozen the same way; the name stays (#4071).
    if let Some((name, rhs)) = solidcraft_engine::doc::expr::definition(text)
        && !rhs.is_empty()
    {
        return format!("{name} = {}", expression(doc, rhs, kind));
    }
    let normalized = doc.with_design_unit(text, kind);
    if kind == Kind::Length && unit(doc) != "mm" && normalized == text && doc.eval(text, Kind::Unitless).is_ok() {
        format!("({text}) * 1 {}", unit(doc))
    } else {
        normalized
    }
}

/// For a field whose command takes only a number, which keeps no expression to name: a typed
/// `name = value` is refused with the way to name it, rather than defining nothing (#4071).
pub(crate) fn no_definition(text: &str, label: &str) -> Result<(), String> {
    match solidcraft_engine::doc::expr::definition(text) {
        Some((name, _)) => Err(format!("{label} keeps no expression, so it cannot define `{name}`: add `{name}` in Parameters, then type `{name}`")),
        None => Ok(()),
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

/// A computed length shown in an editable field or a message, in the design's unit, to a
/// millionth of it with trailing zeros dropped ("3 mm", not "3.000000 mm" or 2.9999999999999996):
/// for a value the field shows but sends only once the user edits it.
pub(crate) fn shown(doc: &Document, mm: f64) -> String {
    let unit = unit(doc);
    let scale = unit_info(unit).map(|(_, scale)| scale).unwrap_or(1.0);
    let text = format!("{:.6}", mm / scale);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{} {unit}", if text == "-0" || text.is_empty() { "0" } else { text })
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
        D::Fillet { radius, distance2, setback, .. } => {
            // A setback not typed (or emptied) stands for the command's default and stays so.
            let mut lengths = vec![radius, distance2];
            if let Some(setback) = setback.as_mut().filter(|s| !s.trim().is_empty()) {
                lengths.push(setback);
            }
            lengths
        }
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
        D::Hole { diameter, depth, cb_diameter, cb_depth, cs_diameter, opts, .. } => {
            vec![diameter, depth, cb_diameter, cb_depth, cs_diameter, &mut opts.bridge]
        }
        D::Primitive { fields, .. } => fields.iter_mut().filter(|(key, _)| *key != "revolutions").map(|(_, value)| value).collect(),
        D::Surface(k) => match k {
            Surf::Extrude { distance, .. } | Surf::Offset { distance, .. } | Surf::Ruled { distance, .. } | Surf::Extend { distance } => {
                vec![distance]
            }
            Surf::Thicken { thickness, .. } => vec![thickness],
            Surf::Stitch { tolerance } | Surf::Fair { tolerance, .. } => vec![tolerance],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_length_keeps_the_size_its_tool_ships_with() {
        // #12418: the number a tool ships with is a size in millimetres, shown in the design's
        // unit: an inch design's extrude of 10 mm starts at "0.4 in", not "10 in".
        for (units, want) in [("mm", "10 mm"), ("cm", "1 cm"), ("m", "0.01 m"), ("in", "0.4 in"), ("ft", "0.033 ft")] {
            let doc = Document { units: units.into(), ..Document::default() };
            assert_eq!(default(&doc, "10"), want, "{units}");
        }
        let doc = Document { units: "in".into(), ..Document::default() };
        assert_eq!(default(&doc, "2"), "0.08 in");
        assert_eq!(default(&doc, "5"), "0.2 in");
        assert_eq!(default(&doc, "0"), "0 in");
    }

    #[test]
    fn a_shown_size_is_the_size_the_tool_ships_with() {
        // A size shown in another unit keeps the size the tool ships with: exactly for a metric
        // unit, within 2% for an imperial one, so the part is the part the tool intends.
        for units in ["mm", "cm", "m", "in", "ft"] {
            let doc = Document { units: units.into(), ..Document::default() };
            for millimetres in ["0", "1", "1.5", "2", "3", "5", "8", "9", "10", "20", "40"] {
                let shown = default(&doc, millimetres);
                assert!(shown.ends_with(&format!(" {units}")), "{shown}");
                let millimetres: f64 = millimetres.parse().unwrap();
                let value = eval(&doc, &shown, Kind::Length).unwrap();
                let exact = matches!(units, "mm" | "cm" | "m");
                let drift = (value - millimetres).abs();
                assert!(drift <= if exact { 1e-9 } else { 0.02 * millimetres }, "{shown} is {value} mm, the tool ships {millimetres} mm");
            }
        }
    }

    #[test]
    fn the_old_default_is_kept_behind_a_preference() {
        // With the preference off, the number the tool ships with is read in the design's units,
        // as before #12418.
        let doc = Document { units: "in".into(), ..Document::default() };
        set_scaled_length_defaults(false);
        assert_eq!(default(&doc, "10"), "10 in");
        set_scaled_length_defaults(true);
        assert_eq!(default(&doc, "10"), "0.4 in");
    }

    #[test]
    fn a_decimal_comma_survives_the_design_unit() {
        // The unit wraps the text in parentheses, where a comma stays decimal (#9408).
        let doc = Document { units: "in".into(), ..Document::default() };
        for (text, inches) in [("2,5", 2.5), ("(2,5)", 2.5), ("-(10-0,5)/2", -4.75), ("2,5 + max(1,2)", 4.5)] {
            let mm = eval(&doc, text, Kind::Length).unwrap();
            assert!((mm - inches * 25.4).abs() < 1e-9, "{text}: {mm}");
        }
    }
}
