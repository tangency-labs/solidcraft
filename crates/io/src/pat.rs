//! Bounded PAT import into kernel-independent hatch records.
//!
//! The public PAT syntax uses degrees and unitless lengths. Import options give those
//! lengths a paper-millimetre meaning; no drawing view scale is applied here. Names,
//! family order, signed angles and dash/gap/dot values are preserved, including repeated
//! names. This reader neither installs a pattern library nor generates hatch geometry.

use solidcraft_geom::Vec2;
use solidcraft_geom::hatch::{Family, MAX_DASHES, MAX_FAMILIES, Pattern};

use crate::{IoError, Result};

const MAX_PAT_BYTES: usize = 1 << 20;
const MAX_PAT_PATTERNS: usize = 1_000;
const MAX_PAT_LINE_CHARS: usize = 80;

/// Length unit of the PAT source, which has no embedded unit declaration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PatUnits {
    #[default]
    Millimetres,
    Inches,
}

/// Convert source lengths to paper millimetres, independently of a drawing view's scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatImportOptions {
    pub units: PatUnits,
    /// Positive finite multiplier, applied once to every origin, delta and dash length.
    pub scale: f64,
}

impl Default for PatImportOptions {
    fn default() -> Self {
        Self { units: PatUnits::Millimetres, scale: 1.0 }
    }
}

/// Read complete PAT definitions in their input order, or report an error without a
/// partial result. Duplicate names, including case variants, remain separate definitions.
///
/// Input must be UTF-8; one initial BOM is accepted. Descriptions and comments may be
/// Unicode, while names and numbers use ASCII syntax. Each physical line is limited to
/// 80 Unicode scalar values before removing comments, excluding CRLF/LF and the initial
/// BOM. The 1 MiB byte limit includes both. Bare CR and embedded BOMs are rejected.
/// At most 1,000 patterns, 64 families per pattern and six dashes per family are admitted.
///
/// A real blank physical line (empty or containing only ASCII spaces/tabs) must follow
/// the final family. A single terminal newline or a comment-only line is insufficient.
/// Errors identify the physical one-based line; invalid options and the total byte cap
/// are import errors without a line. Converted values also pass the hatch validators.
pub fn read_pat(bytes: &[u8], options: PatImportOptions) -> Result<Vec<Pattern>> {
    if bytes.len() > MAX_PAT_BYTES {
        return Err(IoError::Invalid("PAT import: input exceeds 1 MiB".into()));
    }
    let factor = length_factor(options)?;
    let text = std::str::from_utf8(bytes).map_err(|error| {
        let line = 1 + bytes.iter().take(error.valid_up_to()).filter(|byte| **byte == b'\n').count();
        invalid(line, "input is not valid UTF-8")
    })?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut patterns = Vec::new();
    let mut current: Option<(Pattern, usize)> = None;
    let mut final_blank = false;
    let mut physical_lines = 0;

    // split_inclusive does not manufacture a blank record for the last newline.
    for (index, raw) in text.split_inclusive('\n').enumerate() {
        let line_number = index + 1;
        physical_lines = line_number;
        let line = match raw.strip_suffix('\n') {
            Some(line) => line.strip_suffix('\r').unwrap_or(line),
            None => raw,
        };
        check_physical_line(line, line_number)?;
        if trim_horizontal(line).is_empty() {
            final_blank = true;
            continue;
        }
        let content = trim_horizontal(line.split_once(';').map_or(line, |(content, _)| content));
        if content.is_empty() {
            continue;
        }
        final_blank = false;
        if let Some(header) = content.strip_prefix('*') {
            finish_pattern(&mut current, &mut patterns)?;
            if patterns.len() >= MAX_PAT_PATTERNS {
                return Err(invalid(line_number, "more than 1000 patterns"));
            }
            let (name, description) = header.split_once(',').unwrap_or((header, ""));
            current = Some((
                Pattern { name: trim_horizontal(name).into(), description: trim_horizontal(description).into(), families: Vec::new() },
                line_number,
            ));
        } else {
            let (pattern, _) = current.as_mut().ok_or_else(|| invalid(line_number, "family appears before a pattern header"))?;
            if pattern.families.len() >= MAX_FAMILIES {
                return Err(invalid(line_number, "more than 64 families in one pattern"));
            }
            pattern.families.push(read_family(content, line_number, factor)?);
        }
    }
    finish_pattern(&mut current, &mut patterns)?;
    if patterns.is_empty() {
        return Err(invalid(1, "no pattern definitions"));
    }
    if !final_blank {
        return Err(invalid(physical_lines + 1, "missing final blank line after the last family"));
    }
    Ok(patterns)
}

fn length_factor(options: PatImportOptions) -> Result<f64> {
    if !options.scale.is_finite() || options.scale <= 0.0 {
        return Err(IoError::Invalid("PAT import: scale must be positive and finite".into()));
    }
    let unit = match options.units {
        PatUnits::Millimetres => 1.0,
        PatUnits::Inches => 25.4,
    };
    let factor = unit * options.scale;
    if !factor.is_finite() || factor <= 0.0 {
        return Err(IoError::Invalid("PAT import: units and scale produce an invalid length factor".into()));
    }
    Ok(factor)
}

fn trim_horizontal(text: &str) -> &str {
    text.trim_matches([' ', '\t'])
}

fn invalid(line: usize, reason: impl std::fmt::Display) -> IoError {
    IoError::Invalid(format!("PAT line {line}: {reason}"))
}

fn check_physical_line(line: &str, number: usize) -> Result<()> {
    if line.chars().take(MAX_PAT_LINE_CHARS + 1).count() > MAX_PAT_LINE_CHARS {
        return Err(invalid(number, "physical line exceeds 80 characters"));
    }
    if line.contains('\u{feff}') {
        return Err(invalid(number, "a BOM is only allowed once at the start of the file"));
    }
    if line.chars().any(|c| c.is_control() && c != '\t') {
        return Err(invalid(number, "control character or bare CR in physical line"));
    }
    Ok(())
}

fn finish_pattern(current: &mut Option<(Pattern, usize)>, patterns: &mut Vec<Pattern>) -> Result<()> {
    if let Some((pattern, header_line)) = current.take() {
        pattern.validate().map_err(|error| invalid(header_line, error))?;
        patterns.push(pattern);
    }
    Ok(())
}

fn read_family(line: &str, number: usize, factor: f64) -> Result<Family> {
    let mut fields = line.split(',');
    let mut next = |name| -> Result<f64> {
        let field = fields.next().ok_or_else(|| invalid(number, format!("missing {name} field")))?;
        read_number(field, number, name)
    };
    let degrees = next("angle")?;
    let angle = degrees.to_radians();
    if degrees != 0.0 && angle == 0.0 {
        return Err(invalid(number, "angle underflows to zero radians"));
    }
    let origin_x = scaled(next("origin x")?, factor, number, "origin x")?;
    let origin_y = scaled(next("origin y")?, factor, number, "origin y")?;
    let delta_x = scaled(next("delta x")?, factor, number, "delta x")?;
    let delta_y = scaled(next("delta y")?, factor, number, "delta y")?;
    let mut dashes = Vec::new();
    for field in fields {
        if dashes.len() >= MAX_DASHES {
            return Err(invalid(number, "more than 6 dash entries"));
        }
        dashes.push(scaled(read_number(field, number, "dash")?, factor, number, "dash")?);
    }
    let family = Family { angle, origin: Vec2::new(origin_x, origin_y), delta_x, delta_y, dashes };
    family.validate().map_err(|error| invalid(number, error))?;
    Ok(family)
}

fn read_number(field: &str, line: usize, name: &str) -> Result<f64> {
    let token = trim_horizontal(field);
    if !token.is_ascii() || token.is_empty() {
        return Err(invalid(line, format!("{name} needs an ASCII number")));
    }
    let value: f64 = token.parse().map_err(|_| invalid(line, format!("invalid {name} number")))?;
    if !value.is_finite() {
        return Err(invalid(line, format!("{name} must be finite")));
    }
    // A nonzero decimal must not become a dot, zero origin or zero spacing during parsing.
    let mantissa = token.split(['e', 'E']).next().unwrap_or(token);
    if value == 0.0 && mantissa.bytes().any(|b| matches!(b, b'1'..=b'9')) {
        return Err(invalid(line, format!("{name} number underflows to zero")));
    }
    Ok(value)
}

fn scaled(value: f64, factor: f64, line: usize, name: &str) -> Result<f64> {
    let converted = value * factor;
    if !converted.is_finite() {
        return Err(invalid(line, format!("{name} is not finite after unit/scale conversion")));
    }
    if value != 0.0 && converted == 0.0 {
        return Err(invalid(line, format!("{name} underflows to zero after unit/scale conversion")));
    }
    Ok(converted)
}

#[cfg(test)]
#[path = "pat_tests.rs"]
mod tests;
