//! Decoder for first-occurrence embedded schema blobs.

use crate::{BinaryReader, ErrorDetails, ErrorKind, ParseError};

use super::{FieldDefinition, FieldType, SchemaEdit, SchemaResolution, SchemaSource, TypeDefinition};

/// Resource bounds applied while resolving one schema definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaLimits {
    /// Maximum effective fields in one type.
    pub max_fields_per_type: usize,
    /// Maximum bytes in one schema name or description.
    pub max_string_bytes: usize,
    /// Maximum definitions retained by an effective registry.
    pub max_schema_types: usize,
}

impl Default for SchemaLimits {
    fn default() -> Self {
        Self { max_fields_per_type: 4_096, max_string_bytes: 16 * 1024 * 1024, max_schema_types: 65_536 }
    }
}

/// Resolve one embedded schema blob at an absolute input offset.
///
/// `base` must be `Some` exactly when the node type exists in the loaded base
/// schema. A known base type begins with `0xff` or a delta field count; an absent
/// base type begins with a complete field count.
///
/// # Errors
///
/// Returns an offset-bearing error for truncation, invalid field codecs,
/// malformed edit sequences, count mismatches, or exceeded resource bounds.
pub fn decode_embedded_schema(
    data: &[u8],
    offset: usize,
    node_type: u16,
    base: Option<&TypeDefinition>,
    limits: SchemaLimits,
) -> Result<SchemaResolution, ParseError> {
    validate_limits(limits)?;
    if let Some(definition) = base
        && definition.node_type != node_type
    {
        return Err(ParseError::new(
            ErrorKind::InvalidSchemaDefinition,
            offset,
            "base definition node type does not match the requested node type",
            ErrorDetails::NodeType { node_type },
        ));
    }

    let mut reader = BinaryReader::with_position(data, offset)?;
    let marker_offset = reader.position();
    let marker = reader.u8()?;
    let (definition, edits) = match base {
        Some(base_definition) if marker == u8::MAX => {
            let mut definition = base_definition.clone();
            definition.source = SchemaSource::EmbeddedUnchanged;
            (definition, Vec::new())
        }
        Some(base_definition) => decode_delta(&mut reader, node_type, base_definition, usize::from(marker), marker_offset, limits)?,
        None => decode_full(&mut reader, node_type, usize::from(marker), marker_offset, limits)?,
    };

    let end = reader.position();
    Ok(SchemaResolution { definition, raw_schema: data[offset..end].to_vec(), byte_range: offset..end, edits })
}

fn validate_limits(limits: SchemaLimits) -> Result<(), ParseError> {
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

fn decode_full(
    reader: &mut BinaryReader<'_>,
    node_type: u16,
    declared_fields: usize,
    marker_offset: usize,
    limits: SchemaLimits,
) -> Result<(TypeDefinition, Vec<SchemaEdit>), ParseError> {
    enforce_field_limit(declared_fields, marker_offset, limits)?;
    let name_offset = reader.position();
    let name = short_string(reader, "schema_type_name", limits)?;
    if name.is_empty() {
        return Err(invalid_definition(name_offset, node_type, "embedded type name must not be empty"));
    }
    let description = short_string(reader, "schema_type_description", limits)?;
    let mut fields = Vec::with_capacity(declared_fields);
    let mut field_offsets = Vec::with_capacity(declared_fields);
    for _ in 0..declared_fields {
        field_offsets.push(reader.position());
        fields.push(decode_field(reader, node_type, limits)?);
    }
    validate_effective_fields(node_type, &fields, &field_offsets, marker_offset)?;
    Ok((TypeDefinition::from_fields(node_type, name, description, fields, SchemaSource::EmbeddedFull), Vec::new()))
}

fn decode_delta(
    reader: &mut BinaryReader<'_>,
    node_type: u16,
    base: &TypeDefinition,
    declared_fields: usize,
    marker_offset: usize,
    limits: SchemaLimits,
) -> Result<(TypeDefinition, Vec<SchemaEdit>), ParseError> {
    enforce_field_limit(declared_fields, marker_offset, limits)?;
    let mut state = DeltaState::new(base, declared_fields);

    loop {
        let opcode_offset = reader.position();
        let opcode = reader.u8()?;
        if state.apply(reader, node_type, limits, opcode, opcode_offset)? {
            break;
        }
        if state.fields.len() > declared_fields {
            return Err(field_count_mismatch(opcode_offset, declared_fields, state.fields.len()));
        }
    }

    if state.fields.len() != declared_fields {
        return Err(field_count_mismatch(reader.position().saturating_sub(1), declared_fields, state.fields.len()));
    }
    validate_effective_fields(node_type, &state.fields, &state.field_offsets, marker_offset)?;
    Ok((TypeDefinition::from_fields(node_type, base.name.clone(), base.description.clone(), state.fields, SchemaSource::EmbeddedDelta), state.edits))
}

struct DeltaState<'a> {
    base: &'a TypeDefinition,
    fields: Vec<FieldDefinition>,
    field_offsets: Vec<usize>,
    edits: Vec<SchemaEdit>,
    base_index: usize,
    appending: bool,
}

impl<'a> DeltaState<'a> {
    fn new(base: &'a TypeDefinition, declared_fields: usize) -> Self {
        Self {
            base,
            fields: Vec::with_capacity(declared_fields),
            field_offsets: Vec::with_capacity(declared_fields),
            edits: Vec::new(),
            base_index: 0,
            appending: false,
        }
    }

    fn apply(&mut self, reader: &mut BinaryReader<'_>, node_type: u16, limits: SchemaLimits, opcode: u8, offset: usize) -> Result<bool, ParseError> {
        match opcode {
            b'C' => {
                if self.appending || self.base_index >= self.base.fields.len() {
                    return Err(invalid_definition(offset, node_type, "copy instruction has no remaining base field"));
                }
                self.field_offsets.push(offset);
                self.fields.push(self.base.fields[self.base_index].clone());
                self.base_index += 1;
                self.edits.push(SchemaEdit::Copy { offset });
            }
            b'D' => {
                if self.appending || self.base_index >= self.base.fields.len() {
                    return Err(invalid_definition(offset, node_type, "delete instruction has no remaining base field"));
                }
                self.base_index += 1;
                self.edits.push(SchemaEdit::Delete { offset });
            }
            b'I' => {
                if self.appending || self.base_index >= self.base.fields.len() {
                    return Err(invalid_definition(offset, node_type, "insert instruction requires a remaining base field"));
                }
                let field = decode_field(reader, node_type, limits)?;
                self.field_offsets.push(offset);
                self.fields.push(field.clone());
                self.edits.push(SchemaEdit::Insert { offset, field });
            }
            b'A' => {
                if self.base_index < self.base.fields.len() {
                    return Err(invalid_definition(offset, node_type, "append instruction appeared before base fields were exhausted"));
                }
                self.appending = true;
                let field = decode_field(reader, node_type, limits)?;
                self.field_offsets.push(offset);
                self.fields.push(field.clone());
                self.edits.push(SchemaEdit::Append { offset, field });
            }
            b'Z' => {
                self.edits.push(SchemaEdit::End { offset });
                return Ok(true);
            }
            _ => {
                return Err(ParseError::invalid_byte(ErrorKind::UnknownSchemaOpcode, offset, "schema_opcode", opcode));
            }
        }
        Ok(false)
    }
}

fn decode_field(reader: &mut BinaryReader<'_>, node_type: u16, limits: SchemaLimits) -> Result<FieldDefinition, ParseError> {
    let name_offset = reader.position();
    let name = short_string(reader, "schema_field_name", limits)?;
    if name.is_empty() {
        return Err(invalid_definition(name_offset, node_type, "schema field name must not be empty"));
    }
    let pointer_class = reader.u16()?;
    let element_count = reader.positive_integer()?;
    let field_type = if pointer_class == 0 {
        let type_offset = reader.position();
        let type_code = short_string(reader, "schema_field_type", limits)?;
        FieldType::from_code(&type_code).ok_or_else(|| {
            ParseError::new(
                ErrorKind::UnsupportedSchemaFieldType,
                type_offset,
                "embedded schema field uses an unsupported scalar type",
                ErrorDetails::InvalidText { field: "schema_field_type", value: type_code },
            )
        })?
    } else {
        FieldType::PointerIndex
    };
    let transmitted = if element_count == 1 { reader.bool8()? } else { true };
    Ok(FieldDefinition { name, field_type, pointer_class, element_count, transmitted })
}

fn short_string(reader: &mut BinaryReader<'_>, resource: &'static str, limits: SchemaLimits) -> Result<String, ParseError> {
    let length_offset = reader.position();
    let count = usize::from(reader.u8()?);
    if count > limits.max_string_bytes {
        return Err(ParseError::limit(length_offset, resource, count, limits.max_string_bytes));
    }
    reader.ascii(count)
}

fn enforce_field_limit(count: usize, offset: usize, limits: SchemaLimits) -> Result<(), ParseError> {
    if count > limits.max_fields_per_type {
        return Err(ParseError::limit(offset, "schema_fields_per_type", count, limits.max_fields_per_type));
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

    fn positive(value: u32) -> Vec<u8> {
        if value < 32_767 {
            return i16::try_from(value + 1).map_or_else(|_| Vec::new(), |item| item.to_be_bytes().to_vec());
        }
        let quotient = value / 32_767;
        let remainder = value % 32_767 + 1;
        let mut output = i16::try_from(remainder).map_or_else(|_| Vec::new(), |item| (-item).to_be_bytes().to_vec());
        if let Ok(item) = i16::try_from(quotient) {
            output.extend_from_slice(&item.to_be_bytes());
        }
        output
    }

    fn short_string(value: &str) -> Vec<u8> {
        let mut output = Vec::new();
        if let Ok(length) = u8::try_from(value.len()) {
            output.push(length);
            output.extend_from_slice(value.as_bytes());
        }
        output
    }

    fn field(name: &str, pointer_class: u16, element_count: u32, field_type: Option<&str>, transmitted: Option<bool>) -> Vec<u8> {
        let mut output = short_string(name);
        output.extend_from_slice(&pointer_class.to_be_bytes());
        output.extend_from_slice(&positive(element_count));
        if let Some(code) = field_type {
            output.extend_from_slice(&short_string(code));
        }
        if let Some(value) = transmitted {
            output.push(u8::from(value));
        }
        output
    }

    fn base_field(name: &str) -> FieldDefinition {
        FieldDefinition { name: name.to_owned(), field_type: FieldType::Integer, pointer_class: 0, element_count: 0, transmitted: true }
    }

    #[test]
    fn decodes_full_intersection_data_shape() {
        let mut data = vec![2];
        data.extend_from_slice(&short_string("INTERSECTION_DATA"));
        data.extend_from_slice(&short_string("Intersection data"));
        data.extend_from_slice(&field("uv_type", 0, 0, Some("u"), None));
        data.extend_from_slice(&field("values", 0, 1, Some("f"), Some(true)));

        let result = decode_embedded_schema(&data, 0, 204, None, SchemaLimits::default());
        assert!(result.is_ok());
        if let Ok(resolution) = result {
            assert_eq!(resolution.consumed(), 65);
            assert_eq!(resolution.raw_schema, data);
            assert_eq!(resolution.definition.node_type, 204);
            assert_eq!(resolution.definition.name, "INTERSECTION_DATA");
            assert_eq!(resolution.definition.fields.len(), 2);
            assert_eq!(resolution.definition.fields[0].field_type, FieldType::UnsignedByte);
            assert_eq!(resolution.definition.fields[1].element_count, 1);
            assert!(resolution.definition.variable);
            assert_eq!(resolution.definition.source, SchemaSource::EmbeddedFull);
        }
    }

    #[test]
    fn reuses_unchanged_base_definition_for_ff_marker() {
        let base = TypeDefinition::from_fields(12, "BODY", "Body", vec![base_field("node_id")], SchemaSource::Base);
        let result = decode_embedded_schema(&[0xff, 0xaa], 0, 12, Some(&base), SchemaLimits::default());

        assert!(result.is_ok());
        if let Ok(resolution) = result {
            assert_eq!(resolution.consumed(), 1);
            assert_eq!(resolution.definition.fields, base.fields);
            assert_eq!(resolution.definition.source, SchemaSource::EmbeddedUnchanged);
        }
    }

    #[test]
    fn applies_copy_delete_insert_append_and_end_in_order() {
        let base = TypeDefinition::from_fields(29, "POINT", "Point", vec![base_field("a"), base_field("b"), base_field("c")], SchemaSource::Base);
        let mut data = vec![4, b'C', b'D', b'I'];
        data.extend_from_slice(&field("inserted", 0, 0, Some("u"), None));
        data.push(b'C');
        data.push(b'A');
        data.extend_from_slice(&field("appended", 0, 0, Some("f"), None));
        data.push(b'Z');

        let result = decode_embedded_schema(&data, 0, 29, Some(&base), SchemaLimits::default());
        assert!(result.is_ok());
        if let Ok(resolution) = result {
            let names = resolution.definition.fields.iter().map(|item| item.name.as_str()).collect::<Vec<_>>();
            assert_eq!(names, ["a", "inserted", "c", "appended"]);
            assert_eq!(resolution.edits.iter().map(SchemaEdit::opcode).collect::<Vec<_>>(), b"CDICAZ");
            assert_eq!(resolution.definition.source, SchemaSource::EmbeddedDelta);
        }
    }

    #[test]
    fn validates_each_delta_opcode_with_an_independent_fixture() {
        let base = TypeDefinition::from_fields(29, "POINT", "Point", vec![base_field("base")], SchemaSource::Base);

        let copy = decode_embedded_schema(&[1, b'C', b'Z'], 0, 29, Some(&base), SchemaLimits::default());
        assert_eq!(copy.map(|item| item.definition.fields), Ok(vec![base_field("base")]));

        let delete = decode_embedded_schema(&[0, b'D', b'Z'], 0, 29, Some(&base), SchemaLimits::default());
        assert_eq!(delete.map(|item| item.definition.fields), Ok(Vec::new()));

        let mut insert_data = vec![2, b'I'];
        insert_data.extend_from_slice(&field("inserted", 0, 0, Some("u"), None));
        insert_data.extend_from_slice(b"CZ");
        let insert = decode_embedded_schema(&insert_data, 0, 29, Some(&base), SchemaLimits::default());
        assert_eq!(
            insert.map(|item| { item.definition.fields.into_iter().map(|field| field.name).collect::<Vec<_>>() }),
            Ok(vec!["inserted".to_owned(), "base".to_owned()])
        );

        let empty_base = TypeDefinition::from_fields(29, "POINT", "Point", Vec::new(), SchemaSource::Base);
        let mut append_data = vec![1, b'A'];
        append_data.extend_from_slice(&field("appended", 0, 0, Some("f"), None));
        append_data.push(b'Z');
        let append = decode_embedded_schema(&append_data, 0, 29, Some(&empty_base), SchemaLimits::default());
        assert_eq!(
            append.map(|item| item.definition.fields),
            Ok(vec![FieldDefinition {
                name: "appended".to_owned(),
                field_type: FieldType::Double,
                pointer_class: 0,
                element_count: 0,
                transmitted: true,
            }])
        );
    }

    #[test]
    fn reports_unknown_opcode_at_its_absolute_offset() {
        let base = TypeDefinition::from_fields(12, "BODY", "Body", vec![base_field("a")], SchemaSource::Base);
        let data = [0xaa, 0xbb, 1, b'X'];
        let error = decode_embedded_schema(&data, 2, 12, Some(&base), SchemaLimits::default()).err();

        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::UnknownSchemaOpcode));
        assert_eq!(error.as_ref().map(ParseError::offset), Some(3));
    }

    #[test]
    fn rejects_declared_field_count_mismatch() {
        let base = TypeDefinition::from_fields(12, "BODY", "Body", vec![base_field("a")], SchemaSource::Base);
        let error = decode_embedded_schema(&[2, b'C', b'Z'], 0, 12, Some(&base), SchemaLimits::default()).err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::SchemaFieldCountMismatch));
        assert_eq!(error.as_ref().map(ParseError::offset), Some(2));
    }

    #[test]
    fn rejects_unsupported_field_type_at_length_prefix() {
        let mut data = vec![1];
        data.extend_from_slice(&short_string("NEW_TYPE"));
        data.extend_from_slice(&short_string("Description"));
        data.extend_from_slice(&field("value", 0, 0, Some("x"), None));
        let type_offset = data.len() - 2;
        let error = decode_embedded_schema(&data, 0, 200, None, SchemaLimits::default()).err();

        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::UnsupportedSchemaFieldType));
        assert_eq!(error.as_ref().map(ParseError::offset), Some(type_offset));
    }

    #[test]
    fn applies_schema_field_limit_before_allocating() {
        let error = decode_embedded_schema(&[2], 0, 204, None, SchemaLimits { max_fields_per_type: 1, ..SchemaLimits::default() }).err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::LimitExceeded));
        assert_eq!(error.as_ref().map(ParseError::offset), Some(0));
    }
}
