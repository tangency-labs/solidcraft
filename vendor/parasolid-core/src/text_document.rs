//! Strict raw-node parsing for Parasolid text transmit (`X_T`) data.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use crate::header::locate_payload_start;
use crate::schema::{
    EffectiveSchemaRegistry, FieldDefinition, FieldType, SchemaCoverageReport, SchemaEdit, SchemaKey, SchemaLimits, SchemaProvider,
    SchemaProviderResolution, SchemaResolution, SchemaSource, TypeDefinition, embedded_base_definition, missing_type_definition_error,
    unavailable_schema_error, validate_builtin_input,
};
use crate::text_reader::TextReader;
use crate::{DocumentLimits, ErrorDetails, ErrorKind, FieldValue, ParseError, RawField, RawNode};

/// Fields confirmed before decoding an `X_T` node body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XtHeader {
    /// Text transmit format flag; valid files always contain `T`.
    pub flag: u8,
    /// Length-prefixed modeller description from the internal stream.
    pub modeller_version: String,
    /// Length-prefixed schema key from the internal stream.
    pub schema_key: String,
    /// User-field length in integer words, specified as a value from zero to 16.
    pub user_field_size: u8,
    /// Maximum node type present only when the key names an embedded base schema.
    pub schema_max_type: Option<u16>,
    /// Complete input size.
    pub file_size: usize,
    /// Optional common, human-oriented header including its final newline.
    pub common_header_range: Option<Range<usize>>,
    /// Internal `T` header range ending at the first node type.
    pub text_stream_header_range: Range<usize>,
    /// Complete leading header range.
    pub header_range: Range<usize>,
}

/// Validated text termination marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XtTermination {
    /// Decoded index; valid documents always contain zero.
    pub index: u32,
    /// Complete physical source range of the marker.
    pub byte_range: Range<usize>,
}

/// A complete raw `X_T` document ending at a validated terminator.
#[derive(Debug, Clone, PartialEq)]
pub struct XtDocument {
    /// Parsed text header.
    pub header: XtHeader,
    /// Parsed schema-key components.
    pub schema_key: SchemaKey,
    /// Provider provenance retained independently of per-type schema provenance.
    pub schema_provider: SchemaProviderResolution,
    /// Non-termination nodes in source order.
    pub nodes: Vec<RawNode>,
    /// One resolution per encountered node type.
    pub schemas: Vec<SchemaResolution>,
    /// Validated termination record.
    pub terminator: XtTermination,
    /// Effective-schema coverage reached while reading the stream.
    pub schema_coverage: SchemaCoverageReport,
    raw_bytes: Vec<u8>,
}

impl XtDocument {
    /// Borrow the original bytes retained for source provenance.
    #[must_use]
    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }
}

/// Inspect one `X_T` header without decoding its node stream.
///
/// # Errors
///
/// Returns a structured error for an invalid common header, unsupported control byte,
/// malformed text flag, invalid length-prefixed strings, schema key, or user-field size.
pub fn inspect_xt(data: &[u8], limits: crate::InspectionLimits) -> Result<XtHeader, ParseError> {
    read_header(data, limits).map(|(header, _)| header)
}

/// Parse every node in one `X_T` stream using an explicit schema provider.
///
/// # Errors
///
/// Returns a structured error for unavailable schema definitions, malformed text
/// primitives, invalid node framing, exceeded limits, or trailing content.
pub fn parse_xt<P: SchemaProvider>(data: &[u8], provider: &P, limits: DocumentLimits) -> Result<XtDocument, ParseError> {
    let mut nodes = Vec::new();
    let stream = read_xt_stream::<true, _>(data, provider, limits, |node| nodes.push(node))?;
    Ok(XtDocument {
        header: stream.header,
        schema_key: stream.schema_key,
        schema_provider: stream.schema_provider,
        nodes,
        schemas: stream.schemas,
        terminator: stream.terminator,
        schema_coverage: stream.schema_coverage,
        raw_bytes: data.to_vec(),
    })
}

/// Counts returned only after the entire text stream has passed strict parsing.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XtNodeTypeCounts {
    pub record_count: usize,
    pub target_type_counts: BTreeMap<u16, usize>,
}

/// Strict target counting for development tooling without retaining decoded fields.
///
/// Uses the same schema, scalar, index, limit and termination checks as `parse_xt`.
/// Memory still includes the logical text reader, effective schemas and node indices;
/// this is not a constant-memory or schema-independent parser.
///
/// # Errors
///
/// Returns the same parse diagnostics as `parse_xt`; no partial counts are returned.
#[doc(hidden)]
pub fn scan_xt_node_types<P: SchemaProvider>(
    data: &[u8],
    provider: &P,
    limits: DocumentLimits,
    target_node_types: &[u16],
) -> Result<XtNodeTypeCounts, ParseError> {
    let mut target_type_counts = target_node_types.iter().map(|&t| (t, 0)).collect::<BTreeMap<_, _>>();
    let stream = read_xt_stream::<false, _>(data, provider, limits, |node| {
        if let Some(count) = target_type_counts.get_mut(&node.node_type) {
            *count += 1;
        }
    })?;
    Ok(XtNodeTypeCounts { record_count: stream.record_count, target_type_counts })
}

struct XtStreamSummary {
    header: XtHeader,
    schema_key: SchemaKey,
    schema_provider: SchemaProviderResolution,
    schemas: Vec<SchemaResolution>,
    terminator: XtTermination,
    schema_coverage: SchemaCoverageReport,
    record_count: usize,
}

fn read_xt_stream<const RETAIN_FIELDS: bool, P: SchemaProvider>(
    data: &[u8],
    provider: &P,
    limits: DocumentLimits,
    mut visit: impl FnMut(RawNode),
) -> Result<XtStreamSummary, ParseError> {
    validate_limits(limits)?;
    let (header, mut reader) = read_header(data, limits.inspection())?;
    let schema_key = SchemaKey::parse(&header.schema_key)?;
    validate_builtin_input(provider, &schema_key, header.user_field_size, header.header_range.end)?;
    let schema_provider = SchemaProviderResolution::from(provider.provenance());
    let mut registry = EffectiveSchemaRegistry::new(limits.max_schema_types);
    let mut indices = BTreeSet::new();
    let mut record_count = 0_usize;
    loop {
        if reader.is_empty() {
            return Err(ParseError::new(
                ErrorKind::MissingTermination,
                reader.source_position(),
                "text node stream ended before the required termination record",
                ErrorDetails::None,
            ));
        }
        let node_start = reader.source_position();
        let serialized_node_type = reader.short("node_type")?;
        if serialized_node_type == 1 {
            let terminator = read_terminator(&mut reader, node_start)?;
            let schema_coverage = registry.coverage();
            let schemas = registry.resolutions().cloned().collect();
            return Ok(XtStreamSummary { header, schema_key, schema_provider, schemas, terminator, schema_coverage, record_count });
        }
        let node_type = validate_node_type(serialized_node_type, node_start, &header)?;
        let next_count = record_count.saturating_add(1);
        if next_count > limits.max_nodes {
            return Err(ParseError::limit(node_start, "nodes", next_count, limits.max_nodes));
        }
        visit(read_raw_node::<RETAIN_FIELDS, _>(
            data,
            &mut reader,
            &schema_key,
            provider,
            &mut registry,
            &mut indices,
            limits,
            node_type,
            node_start,
            header.user_field_size,
        )?);
        record_count = next_count;
    }
}

fn read_header(data: &[u8], limits: crate::InspectionLimits) -> Result<(XtHeader, TextReader), ParseError> {
    validate_inspection_limits(limits)?;
    if data.len() > limits.max_file_size {
        return Err(ParseError::limit(0, "file_size", data.len(), limits.max_file_size));
    }
    let (payload_start, common_header_range) = locate_payload_start(data, limits.max_string_bytes)?;
    let mut reader = TextReader::new(data, payload_start)?;
    reader.expect(b'T', "text_format_flag", ErrorKind::InvalidTextFlag)?;
    let modeller_version = reader.integer_string("modeller_version", limits.max_string_bytes)?;
    if modeller_version.is_empty() {
        return Err(ParseError::new(
            ErrorKind::InvalidTextToken,
            payload_start + 1,
            "modeller version must not be empty",
            ErrorDetails::InvalidText { field: "modeller_version", value: String::new() },
        ));
    }
    let schema_offset = reader.source_position();
    let schema_key = reader.integer_string("schema_key", limits.max_string_bytes)?;
    let parsed_schema_key = SchemaKey::parse_at(&schema_key, schema_offset)?;
    let schema_max_type = parsed_schema_key.base().map(|_| reader.unsigned_short("schema_max_type")).transpose()?;
    let user_field_offset = reader.source_position();
    let serialized_user_fields = reader.integer("user_field_size")?;
    let user_field_size = u8::try_from(serialized_user_fields).map_err(|_| invalid_user_field_size(user_field_offset, serialized_user_fields))?;
    if user_field_size > 16 {
        return Err(invalid_user_field_size(user_field_offset, serialized_user_fields));
    }
    let body_start = reader.source_position();
    Ok((
        XtHeader {
            flag: b'T',
            modeller_version,
            schema_key,
            user_field_size,
            schema_max_type,
            file_size: data.len(),
            common_header_range,
            text_stream_header_range: payload_start..body_start,
            header_range: 0..body_start,
        },
        reader,
    ))
}

fn invalid_user_field_size(offset: usize, value: i32) -> ParseError {
    ParseError::new(
        ErrorKind::InvalidUserFieldSize,
        offset,
        "user field size must be between 0 and 16 integer words",
        ErrorDetails::InvalidLength { field: "user_field_size", value: i64::from(value) },
    )
}

fn read_terminator(reader: &mut TextReader, node_start: usize) -> Result<XtTermination, ParseError> {
    let index_offset = reader.source_position();
    let index = reader.termination_index("termination_index")?;
    if index != 0 {
        return Err(ParseError::new(
            ErrorKind::InvalidTermination,
            index_offset,
            "termination node must contain index zero",
            ErrorDetails::NodeIndex { node_index: index },
        ));
    }
    let byte_range = node_start..reader.source_position();
    if !reader.is_empty() {
        return Err(ParseError::new(
            ErrorKind::TrailingText,
            reader.source_position(),
            "text remains after the complete termination record",
            ErrorDetails::CountMismatch { field: "trailing_text_characters", expected: 0, actual: reader.remaining() },
        ));
    }
    Ok(XtTermination { index, byte_range })
}

fn validate_node_type(serialized: i16, offset: usize, header: &XtHeader) -> Result<u16, ParseError> {
    let node_type = u16::try_from(serialized).map_err(|_| {
        ParseError::new(
            ErrorKind::InvalidNodeType,
            offset,
            "non-termination node type must be greater than one",
            ErrorDetails::InvalidLength { field: "node_type", value: i64::from(serialized) },
        )
    })?;
    if node_type <= 1 {
        return Err(ParseError::new(
            ErrorKind::InvalidNodeType,
            offset,
            "non-termination node type must be greater than one",
            ErrorDetails::InvalidLength { field: "node_type", value: i64::from(serialized) },
        ));
    }
    if header.schema_max_type.is_some_and(|maximum| node_type > maximum) {
        return Err(ParseError::new(
            ErrorKind::InvalidNodeType,
            offset,
            "node type exceeds the embedded schema header maximum",
            ErrorDetails::NodeType { node_type },
        ));
    }
    Ok(node_type)
}

#[allow(clippy::too_many_arguments)]
fn read_raw_node<const RETAIN_FIELDS: bool, P: SchemaProvider>(
    data: &[u8],
    reader: &mut TextReader,
    schema_key: &SchemaKey,
    provider: &P,
    registry: &mut EffectiveSchemaRegistry,
    indices: &mut BTreeSet<u32>,
    limits: DocumentLimits,
    node_type: u16,
    node_start: usize,
    user_field_size: u8,
) -> Result<RawNode, ParseError> {
    let user_word_count = crate::user_fields::word_count(node_type, user_field_size, node_start, limits.max_variable_elements)?;
    let (definition, first_schema) = resolve_node_definition(data, reader, schema_key, provider, registry, limits, node_type)?;
    let variable_length = if definition.variable {
        let offset = reader.source_position();
        let value = reader.positive_integer("variable_length")?;
        apply_element_limit(offset, value, limits.max_variable_elements)?;
        Some(value)
    } else {
        None
    };
    let index = read_node_index(reader, indices)?;
    let fields = read_node_fields::<RETAIN_FIELDS>(reader, &definition, variable_length, limits)?;
    let mut user_fields = Vec::new();
    for _ in 0..user_word_count {
        let word = reader.nullable_integer("user_field_word")?;
        if RETAIN_FIELDS {
            user_fields.push(word);
        }
    }
    Ok(RawNode { node_type, index, variable_length, definition, first_schema, fields, user_fields, byte_range: node_start..reader.source_position() })
}

#[allow(clippy::too_many_arguments)]
fn resolve_node_definition<P: SchemaProvider>(
    data: &[u8],
    reader: &mut TextReader,
    schema_key: &SchemaKey,
    provider: &P,
    registry: &mut EffectiveSchemaRegistry,
    limits: DocumentLimits,
    node_type: u16,
) -> Result<(TypeDefinition, Option<SchemaResolution>), ParseError> {
    let is_first = registry.get(node_type).is_none();
    if is_first {
        let offset = reader.source_position();
        if !provider.supports_schema_key(schema_key) {
            return Err(unavailable_schema_error(provider, schema_key, node_type, offset));
        }
        let resolution = if schema_key.base().is_some() {
            decode_embedded_schema_text(data, reader, node_type, embedded_base_definition(provider, schema_key, node_type, offset)?, limits.schema())?
        } else {
            let definition = provider
                .type_definition(schema_key.provider_schema(), node_type)
                .ok_or_else(|| missing_type_definition_error(provider, schema_key, node_type, offset))?;
            if definition.fields.len() > limits.max_fields_per_type {
                return Err(ParseError::limit(offset, "schema_fields_per_type", definition.fields.len(), limits.max_fields_per_type));
            }
            let mut definition = definition.clone();
            definition.source = SchemaSource::Base;
            SchemaResolution { definition, raw_schema: Vec::new(), byte_range: offset..offset, edits: Vec::new() }
        };
        registry.insert(resolution)?;
    }
    let resolution = registry.get(node_type).ok_or_else(|| {
        ParseError::new(
            ErrorKind::MissingSchemaType,
            reader.source_position(),
            "effective schema registry did not retain the resolved node type",
            ErrorDetails::NodeType { node_type },
        )
    })?;
    validate_effective_definition(&resolution.definition, reader.source_position())?;
    Ok((resolution.definition.clone(), is_first.then(|| resolution.clone())))
}

fn read_node_index(reader: &mut TextReader, indices: &mut BTreeSet<u32>) -> Result<u32, ParseError> {
    let offset = reader.source_position();
    let index = reader.positive_integer("node_index")?;
    if index == 0 {
        return Err(ParseError::new(
            ErrorKind::InvalidNodeIndex,
            offset,
            "non-termination node index must be greater than zero",
            ErrorDetails::NodeIndex { node_index: index },
        ));
    }
    if !indices.insert(index) {
        return Err(ParseError::new(
            ErrorKind::DuplicateNodeIndex,
            offset,
            "node index was already used by an earlier node",
            ErrorDetails::NodeIndex { node_index: index },
        ));
    }
    Ok(index)
}

fn read_node_fields<const RETAIN_FIELDS: bool>(
    reader: &mut TextReader,
    definition: &TypeDefinition,
    variable_length: Option<u32>,
    limits: DocumentLimits,
) -> Result<Vec<RawField>, ParseError> {
    let mut fields = if RETAIN_FIELDS { Vec::with_capacity(definition.fields.len()) } else { Vec::new() };
    for field in &definition.fields {
        let count = field_value_count(field, variable_length);
        apply_element_limit(reader.source_position(), count, limits.max_variable_elements)?;
        let start = reader.source_position();
        let mut values = Vec::new();
        for _ in 0..count {
            let value = read_field_value(reader, field.field_type)?;
            if RETAIN_FIELDS {
                values.push(value);
            }
        }
        if RETAIN_FIELDS {
            fields.push(RawField { definition: field.clone(), values, byte_range: start..reader.source_position() });
        }
    }
    Ok(fields)
}

fn read_field_value(reader: &mut TextReader, field_type: FieldType) -> Result<FieldValue, ParseError> {
    Ok(match field_type {
        FieldType::UnsignedByte => FieldValue::UnsignedByte(reader.unsigned_byte("unsigned_byte")?),
        FieldType::Character => FieldValue::Character(reader.character()?),
        FieldType::Logical => FieldValue::Logical(reader.logical()?),
        FieldType::ShortInteger => FieldValue::ShortInteger(reader.nullable_short("short_integer")?),
        FieldType::UnicodeCharacter => FieldValue::UnicodeCharacter(reader.unicode_character("unicode_character")?),
        FieldType::Integer => FieldValue::Integer(reader.nullable_integer("integer")?),
        FieldType::PointerIndex => FieldValue::PointerIndex(reader.positive_integer("pointer_index")?),
        FieldType::OpaquePointer => {
            return Err(ParseError::new(
                ErrorKind::UnsupportedSchemaFieldType,
                reader.source_position(),
                "opaque schema pointers have no text transmit representation",
                ErrorDetails::InvalidText { field: "schema_field_type", value: field_type.code().to_owned() },
            ));
        }
        FieldType::Tag => FieldValue::Tag(reader.nullable_integer("tag")?),
        FieldType::Double => FieldValue::Double(reader.nullable_double("double")?),
        FieldType::Interval => FieldValue::Interval(read_nullable_doubles::<2>(reader, "interval")?),
        FieldType::Vector => FieldValue::Vector(read_nullable_doubles::<3>(reader, "vector")?),
        FieldType::Box3 => FieldValue::Box3(read_nullable_doubles::<6>(reader, "box")?),
        FieldType::IntersectionPoint => FieldValue::IntersectionPoint(read_nullable_doubles::<3>(reader, "intersection_point")?),
    })
}

fn read_nullable_doubles<const COUNT: usize>(reader: &mut TextReader, field: &'static str) -> Result<[Option<f64>; COUNT], ParseError> {
    if reader.peek() == Some(b'?') {
        let value = reader.nullable_double(field)?;
        debug_assert!(value.is_none());
        return Ok([None; COUNT]);
    }
    let mut values = [None; COUNT];
    for value in &mut values {
        *value = reader.nullable_double(field)?;
        if value.is_none() {
            return Err(ParseError::new(
                ErrorKind::InvalidTextToken,
                reader.source_position(),
                "a null composite must be encoded by one question mark",
                ErrorDetails::InvalidText { field, value: "?".to_owned() },
            ));
        }
    }
    Ok(values)
}

fn decode_embedded_schema_text(
    data: &[u8],
    reader: &mut TextReader,
    node_type: u16,
    base: Option<&TypeDefinition>,
    limits: SchemaLimits,
) -> Result<SchemaResolution, ParseError> {
    validate_schema_limits(limits)?;
    if let Some(definition) = base
        && definition.node_type != node_type
    {
        return Err(invalid_definition(reader.source_position(), node_type, "base definition node type does not match the requested node type"));
    }
    let start = reader.source_position();
    let marker_offset = start;
    let marker = reader.unsigned_byte("schema_field_count")?;
    let (definition, edits) = match base {
        Some(definition) if marker == u8::MAX => {
            let mut definition = definition.clone();
            definition.source = SchemaSource::EmbeddedUnchanged;
            (definition, Vec::new())
        }
        Some(definition) => decode_delta_text(reader, node_type, definition, usize::from(marker), marker_offset, limits)?,
        None => decode_full_text(reader, node_type, usize::from(marker), marker_offset, limits)?,
    };
    let end = reader.source_position();
    Ok(SchemaResolution { definition, raw_schema: data.get(start..end).unwrap_or_default().to_vec(), byte_range: start..end, edits })
}

fn decode_full_text(
    reader: &mut TextReader,
    node_type: u16,
    declared_fields: usize,
    marker_offset: usize,
    limits: SchemaLimits,
) -> Result<(TypeDefinition, Vec<SchemaEdit>), ParseError> {
    enforce_field_limit(declared_fields, marker_offset, limits)?;
    let name_offset = reader.source_position();
    let name = reader.short_string("schema_type_name", limits.max_string_bytes)?;
    if name.is_empty() {
        return Err(invalid_definition(name_offset, node_type, "embedded type name must not be empty"));
    }
    let description = reader.short_string("schema_type_description", limits.max_string_bytes)?;
    let mut fields = Vec::with_capacity(declared_fields);
    let mut offsets = Vec::with_capacity(declared_fields);
    for _ in 0..declared_fields {
        offsets.push(reader.source_position());
        fields.push(decode_schema_field_text(reader, node_type, limits)?);
    }
    validate_effective_fields(node_type, &fields, &offsets, marker_offset)?;
    Ok((TypeDefinition::from_fields(node_type, name, description, fields, SchemaSource::EmbeddedFull), Vec::new()))
}

fn decode_delta_text(
    reader: &mut TextReader,
    node_type: u16,
    base: &TypeDefinition,
    declared_fields: usize,
    marker_offset: usize,
    limits: SchemaLimits,
) -> Result<(TypeDefinition, Vec<SchemaEdit>), ParseError> {
    enforce_field_limit(declared_fields, marker_offset, limits)?;
    let mut fields = Vec::with_capacity(declared_fields);
    let mut field_offsets = Vec::with_capacity(declared_fields);
    let mut edits = Vec::new();
    let mut base_index = 0_usize;
    let mut appending = false;
    loop {
        let offset = reader.source_position();
        let opcode = reader.raw_character("schema_opcode")?;
        match opcode {
            b'C' => {
                if appending || base_index >= base.fields.len() {
                    return Err(invalid_definition(offset, node_type, "copy instruction has no remaining base field"));
                }
                field_offsets.push(offset);
                fields.push(base.fields[base_index].clone());
                base_index += 1;
                edits.push(SchemaEdit::Copy { offset });
            }
            b'D' => {
                if appending || base_index >= base.fields.len() {
                    return Err(invalid_definition(offset, node_type, "delete instruction has no remaining base field"));
                }
                base_index += 1;
                edits.push(SchemaEdit::Delete { offset });
            }
            b'I' => {
                if appending || base_index >= base.fields.len() {
                    return Err(invalid_definition(offset, node_type, "insert instruction requires a remaining base field"));
                }
                let field = decode_schema_field_text(reader, node_type, limits)?;
                field_offsets.push(offset);
                fields.push(field.clone());
                edits.push(SchemaEdit::Insert { offset, field });
            }
            b'A' => {
                if base_index < base.fields.len() {
                    return Err(invalid_definition(offset, node_type, "append instruction appeared before base fields were exhausted"));
                }
                appending = true;
                let field = decode_schema_field_text(reader, node_type, limits)?;
                field_offsets.push(offset);
                fields.push(field.clone());
                edits.push(SchemaEdit::Append { offset, field });
            }
            b'Z' => {
                edits.push(SchemaEdit::End { offset });
                break;
            }
            _ => {
                return Err(ParseError::invalid_byte(ErrorKind::UnknownSchemaOpcode, offset, "schema_opcode", opcode));
            }
        }
        if fields.len() > declared_fields {
            return Err(field_count_mismatch(offset, declared_fields, fields.len()));
        }
    }
    if fields.len() != declared_fields {
        return Err(field_count_mismatch(reader.source_position(), declared_fields, fields.len()));
    }
    validate_effective_fields(node_type, &fields, &field_offsets, marker_offset)?;
    Ok((TypeDefinition::from_fields(node_type, base.name.clone(), base.description.clone(), fields, SchemaSource::EmbeddedDelta), edits))
}

fn decode_schema_field_text(reader: &mut TextReader, node_type: u16, limits: SchemaLimits) -> Result<FieldDefinition, ParseError> {
    let name_offset = reader.source_position();
    let name = reader.short_string("schema_field_name", limits.max_string_bytes)?;
    if name.is_empty() {
        return Err(invalid_definition(name_offset, node_type, "schema field name must not be empty"));
    }
    let pointer_class = reader.unsigned_short("schema_pointer_class")?;
    let element_count = reader.positive_integer("schema_element_count")?;
    let field_type = if pointer_class == 0 {
        let type_offset = reader.source_position();
        let code = reader.short_string("schema_field_type", limits.max_string_bytes)?;
        FieldType::from_code(&code).ok_or_else(|| {
            ParseError::new(
                ErrorKind::UnsupportedSchemaFieldType,
                type_offset,
                "embedded schema field uses an unsupported scalar type",
                ErrorDetails::InvalidText { field: "schema_field_type", value: code },
            )
        })?
    } else {
        FieldType::PointerIndex
    };
    let transmitted = if element_count == 1 { reader.logical()? } else { true };
    Ok(FieldDefinition { name, field_type, pointer_class, element_count, transmitted })
}

fn validate_inspection_limits(limits: crate::InspectionLimits) -> Result<(), ParseError> {
    if limits.max_file_size == 0 {
        return Err(ParseError::invalid_limit("max_file_size", 0));
    }
    if limits.max_string_bytes == 0 {
        return Err(ParseError::invalid_limit("max_string_bytes", 0));
    }
    Ok(())
}

fn validate_limits(limits: DocumentLimits) -> Result<(), ParseError> {
    for (name, value) in [
        ("max_file_size", limits.max_file_size),
        ("max_nodes", limits.max_nodes),
        ("max_schema_types", limits.max_schema_types),
        ("max_fields_per_type", limits.max_fields_per_type),
        ("max_string_bytes", limits.max_string_bytes),
        ("max_variable_elements", limits.max_variable_elements),
    ] {
        if value == 0 {
            return Err(ParseError::invalid_limit(name, value));
        }
    }
    Ok(())
}

fn validate_schema_limits(limits: SchemaLimits) -> Result<(), ParseError> {
    for (name, value) in [
        ("max_fields_per_type", limits.max_fields_per_type),
        ("max_string_bytes", limits.max_string_bytes),
        ("max_schema_types", limits.max_schema_types),
    ] {
        if value == 0 {
            return Err(ParseError::invalid_limit(name, value));
        }
    }
    Ok(())
}

fn validate_effective_definition(definition: &TypeDefinition, offset: usize) -> Result<(), ParseError> {
    let variable_positions =
        definition.fields.iter().enumerate().filter_map(|(index, field)| (field.element_count == 1).then_some(index)).collect::<Vec<_>>();
    let expected = definition.fields.len().checked_sub(1);
    if variable_positions.len() > 1
        || variable_positions.first().is_some_and(|position| Some(*position) != expected)
        || definition.variable == variable_positions.is_empty()
    {
        return Err(invalid_definition(offset, definition.node_type, "a variable field must be the sole final effective field"));
    }
    Ok(())
}

fn validate_effective_fields(node_type: u16, fields: &[FieldDefinition], offsets: &[usize], fallback_offset: usize) -> Result<(), ParseError> {
    if let Some((index, _)) = fields.iter().enumerate().find(|(index, field)| field.element_count == 1 && *index + 1 != fields.len()) {
        return Err(invalid_definition(
            offsets.get(index).copied().unwrap_or(fallback_offset),
            node_type,
            "only the final effective field may be variable-length",
        ));
    }
    Ok(())
}

fn field_value_count(field: &FieldDefinition, variable_length: Option<u32>) -> u32 {
    if !field.transmitted {
        return 0;
    }
    match field.element_count {
        0 => 1,
        1 => variable_length.unwrap_or(0),
        count => count,
    }
}

fn apply_element_limit(offset: usize, count: u32, limit: usize) -> Result<(), ParseError> {
    let count = usize::try_from(count).map_err(|_| ParseError::limit(offset, "field_elements", usize::MAX, limit))?;
    if count > limit {
        return Err(ParseError::limit(offset, "field_elements", count, limit));
    }
    Ok(())
}

fn enforce_field_limit(count: usize, offset: usize, limits: SchemaLimits) -> Result<(), ParseError> {
    if count > limits.max_fields_per_type {
        return Err(ParseError::limit(offset, "schema_fields_per_type", count, limits.max_fields_per_type));
    }
    Ok(())
}

fn invalid_definition(offset: usize, node_type: u16, message: &str) -> ParseError {
    ParseError::new(ErrorKind::InvalidSchemaDefinition, offset, message, ErrorDetails::NodeType { node_type })
}

fn field_count_mismatch(offset: usize, expected: usize, actual: usize) -> ParseError {
    ParseError::new(
        ErrorKind::SchemaFieldCountMismatch,
        offset,
        "resolved field count does not match the embedded declaration",
        ErrorDetails::CountMismatch { field: "schema_fields", expected, actual },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::InMemorySchemaProvider;

    fn parse_with_scan_parity<P: SchemaProvider>(data: &[u8], provider: &P, limits: DocumentLimits) -> Result<XtDocument, ParseError> {
        let parsed = parse_xt(data, provider, limits);
        let mut targets = BTreeSet::from([110, 111, 112]);
        if let Ok(document) = &parsed {
            targets.extend(document.nodes.iter().map(|node| node.node_type));
        }
        let targets = targets.into_iter().collect::<Vec<_>>();
        let expected = parsed
            .as_ref()
            .map(|document| XtNodeTypeCounts {
                record_count: document.nodes.len(),
                target_type_counts: targets.iter().map(|&t| (t, document.nodes.iter().filter(|node| node.node_type == t).count())).collect(),
            })
            .map_err(Clone::clone);
        assert_eq!(scan_xt_node_types(data, provider, limits, &targets), expected);
        parsed
    }

    fn header(schema: &str) -> Vec<u8> {
        let modeller = ": TRANSMIT FILE created by modeller version 3000000";
        format!("T{} {}{} {}0 ", modeller.len(), modeller, schema.len(), schema).into_bytes()
    }

    fn definition(node_type: u16, fields: Vec<FieldDefinition>) -> TypeDefinition {
        TypeDefinition::from_fields(node_type, "TEST", "Synthetic text node", fields, SchemaSource::Base)
    }

    fn embedded_header(schema: &str, maximum: u16) -> Vec<u8> {
        let mut data = header(schema);
        let standard_suffix = b"0 ";
        assert!(data.ends_with(standard_suffix));
        data.truncate(data.len() - standard_suffix.len());
        data.extend_from_slice(format!("{maximum} 0 ").as_bytes());
        data
    }

    fn field(name: &str, field_type: FieldType, pointer_class: u16, element_count: u32, transmitted: bool) -> FieldDefinition {
        FieldDefinition { name: name.to_owned(), field_type, pointer_class, element_count, transmitted }
    }

    #[test]
    fn inspects_text_header_and_uses_internal_schema_key() {
        let preamble = b"**PART1;FORMAT=text;\n**END_OF_HEADER***\n";
        let mut data = preamble.to_vec();
        data.extend_from_slice(&header("SCH_3000000_30000"));
        data.extend_from_slice(b"1 0 ");
        let result = inspect_xt(&data, crate::InspectionLimits::default());
        assert!(result.is_ok(), "{result:?}");
        if let Ok(header) = result {
            assert_eq!(header.flag, b'T');
            assert_eq!(header.schema_key, "SCH_3000000_30000");
            assert_eq!(header.user_field_size, 0);
            assert_eq!(header.common_header_range, Some(0..preamble.len()));
        }
    }

    #[test]
    fn parses_standard_fields_null_composites_and_terminator() {
        let fields = vec![
            FieldDefinition {
                name: "pointer".to_owned(),
                field_type: FieldType::PointerIndex,
                pointer_class: 12,
                element_count: 0,
                transmitted: true,
            },
            FieldDefinition { name: "sense".to_owned(), field_type: FieldType::Logical, pointer_class: 0, element_count: 0, transmitted: true },
            FieldDefinition { name: "position".to_owned(), field_type: FieldType::Vector, pointer_class: 0, element_count: 0, transmitted: true },
        ];
        let mut provider = InMemorySchemaProvider::new();
        provider.insert("30000", definition(12, fields));
        let mut data = header("SCH_3000000_30000");
        data.extend_from_slice(b"12 1 0 T?1 0 ");
        let result = parse_with_scan_parity(&data, &provider, DocumentLimits::default());
        assert!(result.is_ok(), "{result:?}");
        if let Ok(document) = result {
            assert_eq!(document.schema_provider, SchemaProviderResolution::CallerSupplied);
            assert_eq!(document.nodes.len(), 1);
            assert_eq!(document.nodes[0].fields[0].values[0], FieldValue::PointerIndex(0));
            assert_eq!(document.nodes[0].fields[1].values[0], FieldValue::Logical(true));
            assert_eq!(document.nodes[0].fields[2].values[0], FieldValue::Vector([None; 3]));
            assert_eq!(document.terminator.index, 0);
        }
    }

    #[test]
    fn resolves_a_full_embedded_text_schema() {
        let mut provider = InMemorySchemaProvider::new();
        provider.add_schema("13006");
        let mut data = embedded_header("SCH_3000000_30000_13006", 205);
        // type 204, variable length 2, full schema with one variable double field.
        data.extend_from_slice(b"204 1 4 TEST4 Test6 values0 1 1 fT2 1 1 2 1 0 ");
        let result = parse_with_scan_parity(&data, &provider, DocumentLimits::default());
        assert!(result.is_ok());
        if let Ok(document) = result {
            assert_eq!(document.nodes.len(), 1);
            assert_eq!(document.nodes[0].definition.source, SchemaSource::EmbeddedFull);
            assert_eq!(document.nodes[0].variable_length, Some(2));
            assert_eq!(document.nodes[0].fields[0].values.len(), 2);
        }
    }

    #[test]
    fn resolves_an_unchanged_embedded_text_schema() {
        let fields = vec![field("value", FieldType::Integer, 0, 0, true)];
        let mut provider = InMemorySchemaProvider::new();
        provider.insert("13006", definition(12, fields));
        let mut data = embedded_header("SCH_3000000_30000_13006", 205);
        data.extend_from_slice(b"12 255 1 7 1 0 ");

        let result = parse_with_scan_parity(&data, &provider, DocumentLimits::default());
        assert!(result.is_ok(), "{result:?}");
        if let Ok(document) = result {
            let resolution = document.nodes[0].first_schema.as_ref();
            assert!(resolution.is_some());
            if let Some(resolution) = resolution {
                assert_eq!(resolution.definition.source, SchemaSource::EmbeddedUnchanged);
                assert_eq!(resolution.raw_schema, b"255 ");
            }
            assert_eq!(document.nodes[0].fields[0].values, vec![FieldValue::Integer(Some(7))]);
        }
    }

    #[test]
    fn parses_every_supported_text_field_codec() {
        let fields = vec![
            field("byte", FieldType::UnsignedByte, 0, 0, true),
            field("character", FieldType::Character, 0, 0, true),
            field("logical", FieldType::Logical, 0, 0, true),
            field("short", FieldType::ShortInteger, 0, 0, true),
            field("unicode", FieldType::UnicodeCharacter, 0, 0, true),
            field("integer", FieldType::Integer, 0, 0, true),
            field("pointer", FieldType::PointerIndex, 12, 0, true),
            field("tag", FieldType::Tag, 0, 0, true),
            field("double", FieldType::Double, 0, 0, true),
            field("interval", FieldType::Interval, 0, 0, true),
            field("vector", FieldType::Vector, 0, 0, true),
            field("box", FieldType::Box3, 0, 0, true),
            field("intersection", FieldType::IntersectionPoint, 0, 0, true),
            field("opaque", FieldType::OpaquePointer, 0, 0, false),
        ];
        let mut provider = InMemorySchemaProvider::new();
        provider.insert("30000", definition(12, fields));
        let mut data = header("SCH_3000000_30000");
        data.extend_from_slice(b"12 1 255 \\0T-12 9731 ?0 42 -1.25e+2 ?1 2 3 0 1 2 3 4 5 ?1 0 ");

        let result = parse_with_scan_parity(&data, &provider, DocumentLimits::default());
        assert!(result.is_ok(), "{result:?}");
        if let Ok(document) = result {
            let fields = &document.nodes[0].fields;
            assert_eq!(fields[0].values, vec![FieldValue::UnsignedByte(255)]);
            assert_eq!(fields[1].values, vec![FieldValue::Character(0)]);
            assert_eq!(fields[2].values, vec![FieldValue::Logical(true)]);
            assert_eq!(fields[3].values, vec![FieldValue::ShortInteger(Some(-12))]);
            assert_eq!(fields[4].values, vec![FieldValue::UnicodeCharacter(9_731)]);
            assert_eq!(fields[5].values, vec![FieldValue::Integer(None)]);
            assert_eq!(fields[6].values, vec![FieldValue::PointerIndex(0)]);
            assert_eq!(fields[7].values, vec![FieldValue::Tag(Some(42))]);
            assert_eq!(fields[8].values, vec![FieldValue::Double(Some(-125.0))]);
            assert_eq!(fields[9].values, vec![FieldValue::Interval([None, None])]);
            assert_eq!(fields[10].values, vec![FieldValue::Vector([Some(1.0), Some(2.0), Some(3.0)])]);
            assert_eq!(fields[11].values, vec![FieldValue::Box3([Some(0.0), Some(1.0), Some(2.0), Some(3.0), Some(4.0), Some(5.0),])]);
            assert_eq!(fields[12].values, vec![FieldValue::IntersectionPoint([None, None, None])]);
            assert!(fields[13].values.is_empty());
        }
    }

    #[test]
    fn count_only_stream_does_not_retain_fields_even_within_one_node() -> Result<(), ParseError> {
        let mut provider = InMemorySchemaProvider::new();
        provider.insert("30000", definition(110, vec![field("text", FieldType::Character, 0, 1, true)]));
        let mut data = header("SCH_3000000_30000");
        data.extend_from_slice(b"110 3 1 A\tB110 3 2 XYZ1 0 ");
        let mut visited = 0;
        let summary = read_xt_stream::<false, _>(&data, &provider, DocumentLimits::default(), |node| {
            assert_eq!(node.fields.capacity(), 0);
            assert_eq!(node.node_type, 110);
            visited += 1;
        })?;
        assert_eq!(visited, 2);
        assert_eq!(summary.record_count, 2);
        let parsed = parse_with_scan_parity(&data, &provider, DocumentLimits::default())?;
        assert_eq!(parsed.nodes[0].fields[0].values[1], FieldValue::Character(9));
        assert_eq!(parsed.raw_bytes(), data);
        Ok(())
    }

    #[test]
    fn resolves_every_text_delta_opcode() {
        let base_fields =
            vec![field("a", FieldType::Logical, 0, 0, true), field("b", FieldType::Integer, 0, 0, true), field("c", FieldType::Double, 0, 0, true)];
        let mut provider = InMemorySchemaProvider::new();
        provider.insert("13006", definition(12, base_fields));
        let mut data = embedded_header("SCH_3000000_30000_13006", 205);
        // C(a), D(b), I(inserted), C(c), A(tail), Z; four effective fields.
        data.extend_from_slice(b"12 4 CDI8 inserted0 0 1 dCA4 tail0 2 1 uZ1 T7 2.5 3 4 1 0 ");

        let result = parse_with_scan_parity(&data, &provider, DocumentLimits::default());
        assert!(result.is_ok(), "{result:?}");
        if let Ok(document) = result {
            let resolution = document.nodes[0].first_schema.as_ref();
            assert!(resolution.is_some());
            if let Some(resolution) = resolution {
                assert_eq!(resolution.definition.source, SchemaSource::EmbeddedDelta);
                assert_eq!(
                    resolution.definition.fields.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(),
                    vec!["a", "inserted", "c", "tail"]
                );
                assert_eq!(resolution.edits.len(), 6);
            }
            assert_eq!(document.nodes[0].fields[0].values, vec![FieldValue::Logical(true)]);
            assert_eq!(document.nodes[0].fields[1].values, vec![FieldValue::Integer(Some(7))]);
            assert_eq!(document.nodes[0].fields[2].values, vec![FieldValue::Double(Some(2.5))]);
            assert_eq!(document.nodes[0].fields[3].values, vec![FieldValue::UnsignedByte(3), FieldValue::UnsignedByte(4)]);
        }
    }
}
