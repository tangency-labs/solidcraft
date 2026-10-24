//! Strict parser for the text schema catalogs distributed with Parasolid.

use std::collections::BTreeSet;

use crate::{ErrorDetails, ErrorKind, ParseError};

use super::{FieldDefinition, FieldType, SchemaSource, TypeDefinition};

const SCHEMA_VERSION_PREFIX: &str = ": SCHEMA FILE created by modeller version ";

/// Resource bounds applied while loading one standard/base schema catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaCatalogLimits {
    /// Maximum accepted catalog size in bytes.
    pub max_file_size: usize,
    /// Maximum number of node definitions retained from one catalog.
    pub max_schema_types: usize,
    /// Maximum number of source fields declared by one node definition.
    pub max_fields_per_type: usize,
    /// Maximum bytes in one schema identifier, name, or description.
    pub max_string_bytes: usize,
}

impl Default for SchemaCatalogLimits {
    fn default() -> Self {
        Self { max_file_size: 256 * 1024 * 1024, max_schema_types: 65_536, max_fields_per_type: 4_096, max_string_bytes: 16 * 1024 * 1024 }
    }
}

/// A validated standard schema catalog and its declared provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSchemaCatalog {
    /// Numeric catalog identifier, for example `13006`.
    pub schema_id: String,
    /// Modeller build that created the catalog, for example `1300120`.
    pub modeller_version: String,
    /// Highest node type permitted by the catalog header's type-table range.
    pub declared_max_node_type: u16,
    /// Number of source node records declared by the catalog header.
    pub declared_node_count: usize,
    /// Number of source field records declared by the catalog header.
    pub declared_field_count: usize,
    /// Fourth catalog-header integer, retained without assigning semantics.
    pub declared_auxiliary_count: usize,
    /// Effective definitions, ordered as they occur in the catalog.
    pub definitions: Vec<TypeDefinition>,
}

/// Parse one ASCII `sch_*.sch_txt` standard/base schema catalog.
///
/// Definitions retain all catalog node types. Their field lists contain the
/// effective fields used by XT: transmitted fields plus a variable final field,
/// even when that variable field itself is not transmitted.
///
/// # Errors
///
/// Returns an absolute-offset error for invalid ASCII, malformed records,
/// unsupported field codes, inconsistent counts, duplicate types, or exceeded
/// resource bounds.
pub fn parse_schema_catalog(data: &[u8], limits: SchemaCatalogLimits) -> Result<ParsedSchemaCatalog, ParseError> {
    let text = validate_input(data, limits)?;
    let mut lines = LineCursor::new(text);
    let preamble = parse_preamble(&mut lines, data.len(), limits)?;
    let parsed = parse_definitions(&mut lines, data.len(), &preamble, limits)?;
    validate_declared_counts(&preamble, &parsed)?;
    validate_end_marker(&mut lines, data.len(), &preamble)?;

    Ok(ParsedSchemaCatalog {
        schema_id: preamble.schema_id,
        modeller_version: preamble.modeller_version,
        declared_max_node_type: preamble.max_node_type,
        declared_node_count: preamble.node_count,
        declared_field_count: preamble.field_count,
        declared_auxiliary_count: preamble.auxiliary_count,
        definitions: parsed.definitions,
    })
}

fn validate_input(data: &[u8], limits: SchemaCatalogLimits) -> Result<&str, ParseError> {
    validate_limits(limits)?;
    if data.len() > limits.max_file_size {
        return Err(ParseError::limit(0, "file_size", data.len(), limits.max_file_size));
    }
    if let Some((offset, byte)) = data.iter().copied().enumerate().find(|(_, byte)| !byte.is_ascii()) {
        return Err(ParseError::invalid_byte(ErrorKind::InvalidAscii, offset, "schema_catalog", byte));
    }
    std::str::from_utf8(data).map_err(|_| invalid_catalog(0, "schema catalog is not valid ASCII text", "schema_catalog", "non-UTF-8 input"))
}

struct CatalogPreamble {
    schema_id: String,
    modeller_version: String,
    max_node_type: u16,
    node_count: usize,
    field_count: usize,
    auxiliary_count: usize,
    counts_offset: usize,
}

fn parse_preamble(lines: &mut LineCursor<'_>, data_len: usize, limits: SchemaCatalogLimits) -> Result<CatalogPreamble, ParseError> {
    find_text_header_end(lines, data_len)?;
    require_literal(lines, data_len, "T", "schema format marker")?;
    require_literal(lines, data_len, "1", "schema format version")?;
    let version_line = required_line(lines, data_len, "schema version declaration")?;
    let (modeller_version, schema_id) = parse_version_line(version_line, limits)?;
    let counts_line = required_line(lines, data_len, "schema count declaration")?;
    let counts = parse_count_line(counts_line)?;
    let maximum_plus_one = counts[0];
    if maximum_plus_one == 0 || maximum_plus_one > usize::from(u16::MAX) + 1 {
        return Err(invalid_catalog(
            counts_line.start,
            "declared maximum node type is outside the supported range",
            "schema_max_type_plus_one",
            counts_line.text.trim(),
        ));
    }
    let max_node_type = u16::try_from(maximum_plus_one - 1).map_err(|_| {
        invalid_catalog(
            counts_line.start,
            "declared maximum node type does not fit an unsigned 16-bit value",
            "schema_max_type_plus_one",
            counts_line.text.trim(),
        )
    })?;
    if counts[1] > limits.max_schema_types {
        return Err(ParseError::limit(counts_line.start, "schema_types", counts[1], limits.max_schema_types));
    }
    Ok(CatalogPreamble {
        schema_id,
        modeller_version,
        max_node_type,
        node_count: counts[1],
        field_count: counts[2],
        auxiliary_count: counts[3],
        counts_offset: counts_line.start,
    })
}

struct ParsedDefinitions {
    definitions: Vec<TypeDefinition>,
    source_field_count: usize,
}

fn parse_definitions(
    lines: &mut LineCursor<'_>,
    data_len: usize,
    preamble: &CatalogPreamble,
    limits: SchemaCatalogLimits,
) -> Result<ParsedDefinitions, ParseError> {
    let mut definitions = Vec::with_capacity(preamble.node_count);
    let mut seen_types = BTreeSet::new();
    let mut source_field_count = 0_usize;
    for _ in 0..preamble.node_count {
        let node_line = required_line(lines, data_len, "schema node definition")?;
        let node = parse_node_line(node_line, limits)?;
        validate_node_header(&node, node_line, preamble, limits, &mut seen_types)?;
        source_field_count = source_field_count.checked_add(node.field_count).ok_or_else(|| {
            invalid_catalog(node_line.start, "schema field count overflowed the host size", "schema_field_count", node_line.text.trim())
        })?;
        definitions.push(parse_definition_fields(lines, data_len, node, node_line.start, limits)?);
    }
    Ok(ParsedDefinitions { definitions, source_field_count })
}

fn validate_node_header(
    node: &ParsedNodeLine,
    line: SourceLine<'_>,
    preamble: &CatalogPreamble,
    limits: SchemaCatalogLimits,
    seen_types: &mut BTreeSet<u16>,
) -> Result<(), ParseError> {
    if node.node_type == 0 || node.node_type > preamble.max_node_type {
        return Err(invalid_catalog(line.start, "node type is outside the catalog header range", "schema_node_type", line.text.trim()));
    }
    if !seen_types.insert(node.node_type) {
        return Err(ParseError::new(
            ErrorKind::DuplicateSchemaType,
            line.start,
            "schema catalog contains a duplicate node type",
            ErrorDetails::NodeType { node_type: node.node_type },
        ));
    }
    if node.field_count > limits.max_fields_per_type {
        return Err(ParseError::limit(line.start, "schema_fields_per_type", node.field_count, limits.max_fields_per_type));
    }
    Ok(())
}

fn parse_definition_fields(
    lines: &mut LineCursor<'_>,
    data_len: usize,
    node: ParsedNodeLine,
    node_offset: usize,
    limits: SchemaCatalogLimits,
) -> Result<TypeDefinition, ParseError> {
    let mut effective_fields = Vec::with_capacity(node.field_count);
    let mut effective_offsets = Vec::with_capacity(node.field_count);
    for _ in 0..node.field_count {
        let line = required_line(lines, data_len, "schema field definition")?;
        let field = parse_field_line(line, limits)?;
        if field.transmitted || field.element_count == 1 {
            effective_offsets.push(line.start);
            effective_fields.push(field);
        }
    }
    validate_variable_fields(node.node_type, node.variable, &effective_fields, &effective_offsets, node_offset)?;
    Ok(TypeDefinition::from_fields(node.node_type, node.name, node.description, effective_fields, SchemaSource::Base))
}

fn validate_declared_counts(preamble: &CatalogPreamble, parsed: &ParsedDefinitions) -> Result<(), ParseError> {
    if parsed.source_field_count != preamble.field_count {
        return Err(ParseError::new(
            ErrorKind::SchemaFieldCountMismatch,
            preamble.counts_offset,
            "catalog source-field count does not match its header",
            ErrorDetails::CountMismatch { field: "schema_catalog_fields", expected: preamble.field_count, actual: parsed.source_field_count },
        ));
    }
    Ok(())
}

fn validate_end_marker(lines: &mut LineCursor<'_>, data_len: usize, preamble: &CatalogPreamble) -> Result<(), ParseError> {
    let line = required_line(lines, data_len, "schema end marker")?;
    let expected = format!("**************** end of schema SCH_{}_{} ****************", preamble.modeller_version, preamble.schema_id);
    if line.text.trim() != expected {
        return Err(invalid_catalog(
            line.start,
            "schema end marker does not match the declared catalog version",
            "schema_end_marker",
            line.text.trim(),
        ));
    }
    if let Some(trailing) = lines.next_nonempty() {
        return Err(invalid_catalog(
            trailing.start,
            "non-whitespace content follows the schema end marker",
            "schema_trailing_content",
            trailing.text.trim(),
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct ParsedNodeLine {
    node_type: u16,
    name: String,
    description: String,
    field_count: usize,
    variable: bool,
}

fn parse_node_line(line: SourceLine<'_>, limits: SchemaCatalogLimits) -> Result<ParsedNodeLine, ParseError> {
    let parts = semicolon_parts(line, "schema node definition")?;
    let identity = parts[0].split_whitespace().collect::<Vec<_>>();
    if identity.len() != 2 {
        return Err(invalid_catalog(
            line.start,
            "node definition must begin with a numeric type and name",
            "schema_node_definition",
            line.text.trim(),
        ));
    }
    let node_type = parse_u16(identity[0], line.start, "schema_node_type")?;
    ensure_string_limit(identity[1], line.start, "schema_node_name", limits)?;
    ensure_string_limit(parts[1], line.start, "schema_node_description", limits)?;
    let flags = parts[2].split_whitespace().collect::<Vec<_>>();
    if flags.len() != 3 {
        return Err(invalid_catalog(
            line.start,
            "node definition must contain transmit, field-count, and variable values",
            "schema_node_definition",
            line.text.trim(),
        ));
    }
    let _transmitted = parse_flag(flags[0], line.start, "schema_node_transmit")?;
    let field_count = parse_usize(flags[1], line.start, "schema_node_field_count")?;
    let variable = parse_flag(flags[2], line.start, "schema_node_variable")?;
    Ok(ParsedNodeLine { node_type, name: identity[1].to_owned(), description: parts[1].to_owned(), field_count, variable })
}

fn parse_field_line(line: SourceLine<'_>, limits: SchemaCatalogLimits) -> Result<FieldDefinition, ParseError> {
    let parts = semicolon_parts(line, "schema field definition")?;
    if parts[0].is_empty() || parts[0].split_whitespace().count() != 1 {
        return Err(invalid_catalog(line.start, "schema field name must be one non-empty token", "schema_field_name", parts[0]));
    }
    ensure_string_limit(parts[0], line.start, "schema_field_name", limits)?;
    ensure_string_limit(parts[1], line.start, "schema_field_type", limits)?;
    let field_type = FieldType::from_code(parts[1]).ok_or_else(|| {
        ParseError::new(
            ErrorKind::UnsupportedSchemaFieldType,
            line.start,
            "schema catalog field uses an unsupported type code",
            ErrorDetails::InvalidText { field: "schema_field_type", value: parts[1].to_owned() },
        )
    })?;
    let values = parts[2].split_whitespace().collect::<Vec<_>>();
    if values.len() != 3 {
        return Err(invalid_catalog(
            line.start,
            "field definition must contain transmit, pointer-class, and element-count values",
            "schema_field_definition",
            line.text.trim(),
        ));
    }
    let transmitted = parse_flag(values[0], line.start, "schema_field_transmit")?;
    let pointer_class = parse_u16(values[1], line.start, "schema_pointer_class")?;
    let element_count = parse_u32(values[2], line.start, "schema_element_count")?;
    if pointer_class != 0 && field_type != FieldType::PointerIndex {
        return Err(ParseError::new(
            ErrorKind::InvalidSchemaDefinition,
            line.start,
            "a non-zero pointer class requires field type p",
            ErrorDetails::InvalidText { field: "schema_field_definition", value: line.text.trim().to_owned() },
        ));
    }
    Ok(FieldDefinition { name: parts[0].to_owned(), field_type, pointer_class, element_count, transmitted })
}

fn validate_variable_fields(
    node_type: u16,
    declared_variable: bool,
    fields: &[FieldDefinition],
    offsets: &[usize],
    fallback_offset: usize,
) -> Result<(), ParseError> {
    let variable_positions = fields.iter().enumerate().filter_map(|(index, field)| (field.element_count == 1).then_some(index)).collect::<Vec<_>>();
    let expected = fields.len().checked_sub(1);
    let malformed = variable_positions.len() > 1
        || variable_positions.first().is_some_and(|position| Some(*position) != expected)
        || declared_variable == variable_positions.is_empty();
    if malformed {
        let offset = variable_positions.first().and_then(|position| offsets.get(*position)).copied().unwrap_or(fallback_offset);
        return Err(ParseError::new(
            ErrorKind::InvalidSchemaDefinition,
            offset,
            "catalog variable flag must identify one final effective field",
            ErrorDetails::NodeType { node_type },
        ));
    }
    Ok(())
}

fn parse_version_line(line: SourceLine<'_>, limits: SchemaCatalogLimits) -> Result<(String, String), ParseError> {
    let trimmed = line.text.trim();
    let value = trimmed
        .strip_prefix(SCHEMA_VERSION_PREFIX)
        .and_then(|item| item.strip_suffix(';'))
        .ok_or_else(|| invalid_catalog(line.start, "schema version declaration has an invalid form", "schema_version_declaration", trimmed))?;
    let (modeller, schema) = value.split_once('/').ok_or_else(|| {
        invalid_catalog(line.start, "schema version declaration must contain modeller/schema identifiers", "schema_version_declaration", trimmed)
    })?;
    if !is_numeric(modeller) || !is_numeric(schema) || schema.contains('/') {
        return Err(invalid_catalog(line.start, "schema version identifiers must contain only ASCII digits", "schema_version_declaration", trimmed));
    }
    ensure_string_limit(modeller, line.start, "schema_modeller_version", limits)?;
    ensure_string_limit(schema, line.start, "schema_id", limits)?;
    Ok((modeller.to_owned(), schema.to_owned()))
}

fn parse_count_line(line: SourceLine<'_>) -> Result<[usize; 4], ParseError> {
    let values = line.text.split_whitespace().collect::<Vec<_>>();
    if values.len() != 4 {
        return Err(invalid_catalog(line.start, "schema count declaration must contain four integers", "schema_count_declaration", line.text.trim()));
    }
    Ok([
        parse_usize(values[0], line.start, "schema_max_type_plus_one")?,
        parse_usize(values[1], line.start, "schema_node_count")?,
        parse_usize(values[2], line.start, "schema_field_count")?,
        parse_usize(values[3], line.start, "schema_auxiliary_count")?,
    ])
}

fn semicolon_parts<'a>(line: SourceLine<'a>, record_name: &'static str) -> Result<[&'a str; 3], ParseError> {
    let values = line.text.split(';').map(str::trim).collect::<Vec<_>>();
    if values.len() != 3 {
        return Err(invalid_catalog(line.start, format!("{record_name} must contain exactly two semicolons"), "schema_record", line.text.trim()));
    }
    Ok([values[0], values[1], values[2]])
}

fn parse_flag(value: &str, offset: usize, field: &'static str) -> Result<bool, ParseError> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(invalid_catalog(offset, format!("{field} must be zero or one"), field, value)),
    }
}

fn parse_usize(value: &str, offset: usize, field: &'static str) -> Result<usize, ParseError> {
    value.parse::<usize>().map_err(|_| invalid_catalog(offset, format!("{field} must be a non-negative integer"), field, value))
}

fn parse_u16(value: &str, offset: usize, field: &'static str) -> Result<u16, ParseError> {
    value.parse::<u16>().map_err(|_| invalid_catalog(offset, format!("{field} must fit an unsigned 16-bit integer"), field, value))
}

fn parse_u32(value: &str, offset: usize, field: &'static str) -> Result<u32, ParseError> {
    value.parse::<u32>().map_err(|_| invalid_catalog(offset, format!("{field} must fit an unsigned 32-bit integer"), field, value))
}

fn ensure_string_limit(value: &str, offset: usize, resource: &'static str, limits: SchemaCatalogLimits) -> Result<(), ParseError> {
    if value.len() > limits.max_string_bytes {
        return Err(ParseError::limit(offset, resource, value.len(), limits.max_string_bytes));
    }
    Ok(())
}

fn is_numeric(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn validate_limits(limits: SchemaCatalogLimits) -> Result<(), ParseError> {
    for (name, value) in [
        ("max_file_size", limits.max_file_size),
        ("max_schema_types", limits.max_schema_types),
        ("max_fields_per_type", limits.max_fields_per_type),
        ("max_string_bytes", limits.max_string_bytes),
    ] {
        if value == 0 {
            return Err(ParseError::invalid_limit(name, value));
        }
    }
    Ok(())
}

fn find_text_header_end(lines: &mut LineCursor<'_>, data_len: usize) -> Result<(), ParseError> {
    while let Some(line) = lines.next_line() {
        if line.text.trim_start().starts_with("**END_OF_HEADER") {
            return Ok(());
        }
    }
    Err(ParseError::new(
        ErrorKind::MissingTextHeaderTerminator,
        data_len,
        "schema catalog text header has no END_OF_HEADER marker",
        ErrorDetails::None,
    ))
}

fn require_literal(lines: &mut LineCursor<'_>, data_len: usize, expected: &str, name: &'static str) -> Result<(), ParseError> {
    let line = required_line(lines, data_len, name)?;
    if line.text.trim() != expected {
        return Err(invalid_catalog(line.start, format!("{name} must be {expected:?}"), "schema_preamble", line.text.trim()));
    }
    Ok(())
}

fn required_line<'a>(lines: &mut LineCursor<'a>, data_len: usize, name: &'static str) -> Result<SourceLine<'a>, ParseError> {
    lines.next_nonempty().ok_or_else(|| invalid_catalog(data_len, format!("schema catalog ended before {name}"), "schema_catalog", "end of file"))
}

fn invalid_catalog(offset: usize, message: impl Into<String>, field: &'static str, value: impl Into<String>) -> ParseError {
    ParseError::new(ErrorKind::InvalidSchemaCatalog, offset, message, ErrorDetails::InvalidText { field, value: value.into() })
}

#[derive(Debug, Clone, Copy)]
struct SourceLine<'a> {
    start: usize,
    text: &'a str,
}

struct LineCursor<'a> {
    text: &'a str,
    position: usize,
}

impl<'a> LineCursor<'a> {
    const fn new(text: &'a str) -> Self {
        Self { text, position: 0 }
    }

    fn next_line(&mut self) -> Option<SourceLine<'a>> {
        if self.position >= self.text.len() {
            return None;
        }
        let start = self.position;
        let remaining = &self.text[start..];
        let relative_end = remaining.find('\n').unwrap_or(remaining.len());
        let mut end = start + relative_end;
        self.position = if end < self.text.len() { end + 1 } else { self.text.len() };
        if end > start && self.text.as_bytes()[end - 1] == b'\r' {
            end -= 1;
        }
        Some(SourceLine { start, text: &self.text[start..end] })
    }

    fn next_nonempty(&mut self) -> Option<SourceLine<'a>> {
        while let Some(line) = self.next_line() {
            if !line.text.trim().is_empty() {
                return Some(line);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_catalog() -> Vec<u8> {
        b"**PARASOLID schema fixture\r\n\
**END_OF_HEADER***************************************************\r\n\
T\r\n\
1\r\n\
: SCHEMA FILE created by modeller version 3000119/30000;\r\n\
3 2 3 17\r\n\
1 NULLP; Null; 1 0 0\r\n\
2 VALUES; Values; 1 3 1\r\n\
hidden; d; 0 0 0\r\n\
tag; t; 1 0 0\r\n\
raw; q; 0 0 1\r\n\
**************** end of schema SCH_3000119_30000 ****************\r\n"
            .to_vec()
    }

    #[test]
    fn parses_catalog_metadata_and_effective_fields() {
        let parsed = parse_schema_catalog(&valid_catalog(), SchemaCatalogLimits::default());
        assert!(parsed.is_ok());
        if let Ok(catalog) = parsed {
            assert_eq!(catalog.schema_id, "30000");
            assert_eq!(catalog.modeller_version, "3000119");
            assert_eq!(catalog.declared_max_node_type, 2);
            assert_eq!(catalog.declared_node_count, 2);
            assert_eq!(catalog.declared_field_count, 3);
            assert_eq!(catalog.declared_auxiliary_count, 17);
            assert_eq!(catalog.definitions.len(), 2);
            assert_eq!(catalog.definitions[1].fields.len(), 2);
            assert_eq!(catalog.definitions[1].fields[0].field_type, FieldType::Tag);
            assert_eq!(catalog.definitions[1].fields[1].field_type, FieldType::OpaquePointer);
            assert!(!catalog.definitions[1].fields[1].transmitted);
            assert!(catalog.definitions[1].variable);
        }
    }

    #[test]
    fn rejects_inconsistent_variable_flag_with_a_source_offset() {
        let data = String::from_utf8(valid_catalog()).unwrap_or_default();
        let data = data.replace("2 VALUES; Values; 1 3 1", "2 VALUES; Values; 1 3 0");
        let error = parse_schema_catalog(data.as_bytes(), SchemaCatalogLimits::default()).err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidSchemaDefinition));
        assert!(error.is_some_and(|item| item.offset() > 0));
    }

    #[test]
    fn accepts_a_declared_node_type_range_larger_than_the_observed_maximum() {
        let data = String::from_utf8(valid_catalog()).unwrap_or_default();
        let data = data.replace("3 2 3 17", "120 2 3 17");
        let parsed = parse_schema_catalog(data.as_bytes(), SchemaCatalogLimits::default());
        assert!(parsed.is_ok());
        assert_eq!(parsed.map(|catalog| catalog.declared_max_node_type), Ok(119));
    }

    #[test]
    fn rejects_count_limit_ascii_and_trailing_content() {
        let type_limit = parse_schema_catalog(&valid_catalog(), SchemaCatalogLimits { max_schema_types: 1, ..SchemaCatalogLimits::default() }).err();
        assert_eq!(type_limit.as_ref().map(ParseError::kind), Some(ErrorKind::LimitExceeded));

        let mut non_ascii = valid_catalog();
        non_ascii.push(0x80);
        let ascii_error = parse_schema_catalog(&non_ascii, SchemaCatalogLimits::default()).err();
        assert_eq!(ascii_error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidAscii));
        assert_eq!(ascii_error.as_ref().map(ParseError::offset), Some(non_ascii.len() - 1));

        let mut trailing = valid_catalog();
        trailing.extend_from_slice(b"unexpected\n");
        let trailing_error = parse_schema_catalog(&trailing, SchemaCatalogLimits::default()).err();
        assert_eq!(trailing_error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidSchemaCatalog));
    }
}
