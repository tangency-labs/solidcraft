//! Strict raw-node parsing and byte-exact reconstruction for neutral `X_B` data.

use std::collections::BTreeSet;
use std::ops::Range;

use crate::schema::{
    EffectiveSchemaRegistry, FieldDefinition, FieldType, SchemaCoverageReport, SchemaKey, SchemaLimits, SchemaProvider, SchemaProviderResolution,
    SchemaResolution, TypeDefinition, validate_builtin_input,
};
use crate::{BinaryReader, ErrorDetails, ErrorKind, InspectionLimits, ParseError, XbHeader, inspect_xb};

/// Resource bounds applied while framing a complete node stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentLimits {
    /// Maximum accepted source size in bytes.
    pub max_file_size: usize,
    /// Maximum number of non-termination nodes.
    pub max_nodes: usize,
    /// Maximum retained effective schema definitions.
    pub max_schema_types: usize,
    /// Maximum effective fields in one type definition.
    pub max_fields_per_type: usize,
    /// Maximum header or embedded-schema string length.
    pub max_string_bytes: usize,
    /// Maximum number of values decoded for one array field.
    pub max_variable_elements: usize,
}

impl Default for DocumentLimits {
    fn default() -> Self {
        Self {
            max_file_size: 256 * 1024 * 1024,
            max_nodes: 10_000_000,
            max_schema_types: 65_536,
            max_fields_per_type: 4_096,
            max_string_bytes: 16 * 1024 * 1024,
            max_variable_elements: 10_000_000,
        }
    }
}

impl DocumentLimits {
    pub(crate) const fn inspection(self) -> InspectionLimits {
        InspectionLimits { max_file_size: self.max_file_size, max_string_bytes: self.max_string_bytes }
    }

    pub(crate) const fn schema(self) -> SchemaLimits {
        SchemaLimits {
            max_fields_per_type: self.max_fields_per_type,
            max_string_bytes: self.max_string_bytes,
            max_schema_types: self.max_schema_types,
        }
    }
}

/// One scalar or fixed composite decoded according to an effective schema field.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    /// `u`.
    UnsignedByte(u8),
    /// `c`, retained as its physical byte.
    Character(u8),
    /// `l`.
    Logical(bool),
    /// `n`, with the documented unset sentinel represented by `None`.
    ShortInteger(Option<i16>),
    /// `w`, retained as one UTF-16 code unit.
    UnicodeCharacter(u16),
    /// `d`, with the documented unset sentinel represented by `None`.
    Integer(Option<i32>),
    /// `p`.
    PointerIndex(u32),
    /// `t`, physically encoded as a signed integer.
    Tag(Option<i32>),
    /// `f`, with the documented unset sentinel represented by `None`.
    Double(Option<f64>),
    /// `i`.
    Interval([Option<f64>; 2]),
    /// `v`.
    Vector([Option<f64>; 3]),
    /// `b`.
    Box3([Option<f64>; 6]),
    /// `h`; only its transmitted position vector is present.
    IntersectionPoint([Option<f64>; 3]),
}

impl FieldValue {
    /// Return the schema codec which produced this value.
    #[must_use]
    pub const fn field_type(&self) -> FieldType {
        match self {
            Self::UnsignedByte(_) => FieldType::UnsignedByte,
            Self::Character(_) => FieldType::Character,
            Self::Logical(_) => FieldType::Logical,
            Self::ShortInteger(_) => FieldType::ShortInteger,
            Self::UnicodeCharacter(_) => FieldType::UnicodeCharacter,
            Self::Integer(_) => FieldType::Integer,
            Self::PointerIndex(_) => FieldType::PointerIndex,
            Self::Tag(_) => FieldType::Tag,
            Self::Double(_) => FieldType::Double,
            Self::Interval(_) => FieldType::Interval,
            Self::Vector(_) => FieldType::Vector,
            Self::Box3(_) => FieldType::Box3,
            Self::IntersectionPoint(_) => FieldType::IntersectionPoint,
        }
    }
}

/// One decoded effective field and its exact source range.
#[derive(Debug, Clone, PartialEq)]
pub struct RawField {
    /// Effective schema definition used for this field.
    pub definition: FieldDefinition,
    /// Scalar or array values in transmit order.
    pub values: Vec<FieldValue>,
    /// Half-open range containing only the transmitted field values.
    pub byte_range: Range<usize>,
}

/// One non-termination node in source order.
#[derive(Debug, Clone, PartialEq)]
pub struct RawNode {
    /// Numeric node type.
    pub node_type: u16,
    /// Non-zero transmit index.
    pub index: u32,
    /// Per-node final-field length for a variable type.
    pub variable_length: Option<u32>,
    /// Effective definition used to frame this node.
    pub definition: TypeDefinition,
    /// First-occurrence schema resolution; absent on subsequent nodes of the type.
    pub first_schema: Option<SchemaResolution>,
    /// Decoded effective fields.
    pub fields: Vec<RawField>,
    /// Complete record range, including any trailing user-field words.
    pub byte_range: Range<usize>,
    /// Application-owned integer words, separate from effective schema fields.
    /// The documented integer unset sentinel is retained as `None`.
    pub user_fields: Vec<Option<i32>>,
}

/// Validated end marker for a complete node stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XbTermination {
    /// Decoded compact index; valid documents always contain zero.
    pub index: u32,
    /// Range containing node type 1 and its compact zero index.
    pub byte_range: Range<usize>,
}

/// A complete neutral-binary raw document ending at a validated terminator.
#[derive(Debug, Clone, PartialEq)]
pub struct XbDocument {
    /// Parsed binary header.
    pub header: XbHeader,
    /// Parsed schema-key components.
    pub schema_key: SchemaKey,
    /// Provider provenance retained independently of per-type schema provenance.
    pub schema_provider: SchemaProviderResolution,
    /// Non-termination nodes in source order.
    pub nodes: Vec<RawNode>,
    /// One resolution per encountered node type.
    pub schemas: Vec<SchemaResolution>,
    /// Validated termination record.
    pub terminator: XbTermination,
    /// Effective-schema coverage reached while reading the stream.
    pub schema_coverage: SchemaCoverageReport,
    raw_bytes: Vec<u8>,
}

impl XbDocument {
    /// Borrow the original bytes retained for lossless reconstruction.
    #[must_use]
    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }
}

/// Parse every node in one neutral `X_B` payload using an explicit base-schema provider.
///
/// # Errors
///
/// Returns a structured error for invalid framing, unavailable schema definitions,
/// unsupported user fields, limit violations, a missing or invalid terminator, or
/// trailing bytes.
pub fn parse_xb<P: SchemaProvider>(data: &[u8], provider: &P, limits: DocumentLimits) -> Result<XbDocument, ParseError> {
    validate_limits(limits)?;
    let header = inspect_xb(data, limits.inspection())?;
    let schema_key = SchemaKey::parse(&header.schema_key)?;
    validate_builtin_input(provider, &schema_key, header.user_field_size, header.header_range.end)?;
    let schema_provider = SchemaProviderResolution::from(provider.provenance());
    let mut reader = BinaryReader::with_position(data, header.header_range.end)?;
    let mut registry = EffectiveSchemaRegistry::new(limits.max_schema_types);
    let mut indices = BTreeSet::new();
    let mut nodes = Vec::new();

    loop {
        require_node_or_termination(&reader)?;
        let node_start = reader.position();
        let serialized_node_type = reader.i16()?;
        if serialized_node_type == 1 {
            let terminator = read_terminator(&mut reader, node_start)?;
            let schema_coverage = registry.coverage();
            let schemas = registry.resolutions().cloned().collect();
            return Ok(XbDocument { header, schema_key, schema_provider, nodes, schemas, terminator, schema_coverage, raw_bytes: data.to_vec() });
        }
        let node_type = validate_node_type(serialized_node_type, node_start, &header)?;
        let next_count = nodes.len().saturating_add(1);
        if next_count > limits.max_nodes {
            return Err(ParseError::limit(node_start, "nodes", next_count, limits.max_nodes));
        }
        nodes.push(read_raw_node(
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
    }
}

fn require_node_or_termination(reader: &BinaryReader<'_>) -> Result<(), ParseError> {
    if reader.remaining() == 0 {
        return Err(ParseError::new(
            ErrorKind::MissingTermination,
            reader.position(),
            "node stream ended before the required termination record",
            ErrorDetails::None,
        ));
    }
    Ok(())
}

fn read_terminator(reader: &mut BinaryReader<'_>, node_start: usize) -> Result<XbTermination, ParseError> {
    let index_offset = reader.position();
    let index = reader.positive_integer()?;
    if index != 0 {
        return Err(ParseError::new(
            ErrorKind::InvalidTermination,
            index_offset,
            "termination node must contain index zero",
            ErrorDetails::NodeIndex { node_index: index },
        ));
    }
    let byte_range = node_start..reader.position();
    if reader.remaining() != 0 {
        return Err(ParseError::new(
            ErrorKind::TrailingBytes,
            reader.position(),
            "bytes remain after the complete termination record",
            ErrorDetails::CountMismatch { field: "trailing_bytes", expected: 0, actual: reader.remaining() },
        ));
    }
    Ok(XbTermination { index, byte_range })
}

fn validate_node_type(serialized: i16, offset: usize, header: &XbHeader) -> Result<u16, ParseError> {
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
fn read_raw_node<P: SchemaProvider>(
    data: &[u8],
    reader: &mut BinaryReader<'_>,
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
    let variable_length = read_variable_length(reader, &definition, limits)?;
    let index = read_node_index(reader, indices)?;
    let fields = read_node_fields(reader, &definition, variable_length, limits)?;
    let user_fields = (0..user_word_count).map(|_| reader.nullable_i32()).collect::<Result<Vec<_>, _>>()?;
    Ok(RawNode { node_type, index, variable_length, definition, first_schema, fields, user_fields, byte_range: node_start..reader.position() })
}

#[allow(clippy::too_many_arguments)]
fn resolve_node_definition<P: SchemaProvider>(
    data: &[u8],
    reader: &mut BinaryReader<'_>,
    schema_key: &SchemaKey,
    provider: &P,
    registry: &mut EffectiveSchemaRegistry,
    limits: DocumentLimits,
    node_type: u16,
) -> Result<(TypeDefinition, Option<SchemaResolution>), ParseError> {
    let is_first = registry.get(node_type).is_none();
    if is_first {
        let schema_offset = reader.position();
        let consumed = registry.resolve_first(data, schema_offset, node_type, schema_key, provider, limits.schema())?;
        reader.bytes(consumed)?;
    }
    let resolution = registry.get(node_type).ok_or_else(|| {
        ParseError::new(
            ErrorKind::MissingSchemaType,
            reader.position(),
            "effective schema registry did not retain the resolved node type",
            ErrorDetails::NodeType { node_type },
        )
    })?;
    validate_effective_definition(&resolution.definition, reader.position())?;
    Ok((resolution.definition.clone(), is_first.then(|| resolution.clone())))
}

fn read_variable_length(reader: &mut BinaryReader<'_>, definition: &TypeDefinition, limits: DocumentLimits) -> Result<Option<u32>, ParseError> {
    if !definition.variable {
        return Ok(None);
    }
    let offset = reader.position();
    let serialized = reader.i32()?;
    let value = u32::try_from(serialized).map_err(|_| {
        ParseError::new(
            ErrorKind::InvalidVariableLength,
            offset,
            "variable field length must be non-negative",
            ErrorDetails::InvalidLength { field: "variable_length", value: i64::from(serialized) },
        )
    })?;
    apply_element_limit(offset, value, limits.max_variable_elements)?;
    Ok(Some(value))
}

fn read_node_index(reader: &mut BinaryReader<'_>, indices: &mut BTreeSet<u32>) -> Result<u32, ParseError> {
    let offset = reader.position();
    let index = reader.positive_integer()?;
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

fn read_node_fields(
    reader: &mut BinaryReader<'_>,
    definition: &TypeDefinition,
    variable_length: Option<u32>,
    limits: DocumentLimits,
) -> Result<Vec<RawField>, ParseError> {
    let mut fields = Vec::with_capacity(definition.fields.len());
    for field in &definition.fields {
        let count = field_value_count(field, variable_length);
        apply_element_limit(reader.position(), count, limits.max_variable_elements)?;
        let start = reader.position();
        let mut values = Vec::new();
        for _ in 0..count {
            values.push(read_field_value(reader, field.field_type)?);
        }
        fields.push(RawField { definition: field.clone(), values, byte_range: start..reader.position() });
    }
    Ok(fields)
}

/// Reconstruct an unmodified parsed document byte-for-byte.
///
/// v0.1 intentionally does not re-encode edited nodes or synthesize schemas.
#[must_use]
pub fn write_xb(document: &XbDocument) -> Vec<u8> {
    document.raw_bytes.clone()
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

fn validate_effective_definition(definition: &TypeDefinition, offset: usize) -> Result<(), ParseError> {
    let variable_positions =
        definition.fields.iter().enumerate().filter_map(|(index, field)| (field.element_count == 1).then_some(index)).collect::<Vec<_>>();
    let expected = definition.fields.len().checked_sub(1);
    if variable_positions.len() > 1
        || variable_positions.first().is_some_and(|position| Some(*position) != expected)
        || definition.variable == variable_positions.is_empty()
    {
        return Err(ParseError::new(
            ErrorKind::InvalidSchemaDefinition,
            offset,
            "a variable field must be the sole final effective field",
            ErrorDetails::NodeType { node_type: definition.node_type },
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

fn read_field_value(reader: &mut BinaryReader<'_>, field_type: FieldType) -> Result<FieldValue, ParseError> {
    Ok(match field_type {
        FieldType::UnsignedByte => FieldValue::UnsignedByte(reader.u8()?),
        FieldType::Character => FieldValue::Character(reader.u8()?),
        FieldType::Logical => FieldValue::Logical(reader.bool8()?),
        FieldType::ShortInteger => FieldValue::ShortInteger(reader.nullable_i16()?),
        FieldType::UnicodeCharacter => FieldValue::UnicodeCharacter(reader.u16()?),
        FieldType::Integer => FieldValue::Integer(reader.nullable_i32()?),
        FieldType::PointerIndex => FieldValue::PointerIndex(reader.positive_integer()?),
        FieldType::OpaquePointer => {
            return Err(ParseError::new(
                ErrorKind::UnsupportedSchemaFieldType,
                reader.position(),
                "opaque schema pointers have no neutral transmit representation",
                ErrorDetails::InvalidText { field: "schema_field_type", value: field_type.code().to_owned() },
            ));
        }
        FieldType::Tag => FieldValue::Tag(reader.nullable_i32()?),
        FieldType::Double => FieldValue::Double(reader.nullable_f64()?),
        FieldType::Interval => FieldValue::Interval(read_nullable_doubles::<2>(reader)?),
        FieldType::Vector => FieldValue::Vector(read_nullable_doubles::<3>(reader)?),
        FieldType::Box3 => FieldValue::Box3(read_nullable_doubles::<6>(reader)?),
        FieldType::IntersectionPoint => FieldValue::IntersectionPoint(read_nullable_doubles::<3>(reader)?),
    })
}

fn read_nullable_doubles<const COUNT: usize>(reader: &mut BinaryReader<'_>) -> Result<[Option<f64>; COUNT], ParseError> {
    let mut values = [None; COUNT];
    for value in &mut values {
        *value = reader.nullable_f64()?;
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{InMemorySchemaProvider, SchemaSource};

    fn append_i16(output: &mut Vec<u8>, value: i16) {
        output.extend_from_slice(&value.to_be_bytes());
    }

    fn append_i32(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&value.to_be_bytes());
    }

    fn append_positive(output: &mut Vec<u8>, value: u32) {
        if value < 32_767 {
            append_i16(output, i16::try_from(value + 1).unwrap_or(i16::MAX));
            return;
        }
        let quotient = value / 32_767;
        let remainder = value % 32_767;
        append_i16(output, -i16::try_from(remainder + 1).unwrap_or(i16::MAX));
        append_i16(output, i16::try_from(quotient).unwrap_or(i16::MAX));
    }

    fn header(schema: &str, maximum: Option<u16>, user_fields: i32) -> Vec<u8> {
        let modeller = b": TRANSMIT FILE created by modeller version 3000000";
        let mut output = b"PS\0\0".to_vec();
        output.extend_from_slice(&u16::try_from(modeller.len()).unwrap_or_default().to_be_bytes());
        output.extend_from_slice(modeller);
        output.extend_from_slice(&i32::try_from(schema.len()).unwrap_or_default().to_be_bytes());
        output.extend_from_slice(schema.as_bytes());
        if let Some(maximum) = maximum {
            output.extend_from_slice(&maximum.to_be_bytes());
        }
        output.extend_from_slice(&user_fields.to_be_bytes());
        output
    }

    fn field(name: &str, field_type: FieldType, element_count: u32) -> FieldDefinition {
        FieldDefinition {
            name: name.to_owned(),
            field_type,
            pointer_class: u16::from(field_type == FieldType::PointerIndex),
            element_count,
            transmitted: true,
        }
    }

    fn standard_provider() -> InMemorySchemaProvider {
        let mut provider = InMemorySchemaProvider::new();
        provider.insert(
            "30000",
            TypeDefinition::from_fields(
                42,
                "EVERY_FIXED_CODEC",
                "Every fixed codec",
                vec![
                    field("u", FieldType::UnsignedByte, 0),
                    field("c", FieldType::Character, 0),
                    field("l", FieldType::Logical, 0),
                    field("n", FieldType::ShortInteger, 0),
                    field("w", FieldType::UnicodeCharacter, 0),
                    field("d", FieldType::Integer, 0),
                    field("p", FieldType::PointerIndex, 0),
                    field("f", FieldType::Double, 0),
                    field("i", FieldType::Interval, 0),
                    field("v", FieldType::Vector, 0),
                    field("b", FieldType::Box3, 0),
                    field("h", FieldType::IntersectionPoint, 0),
                ],
                SchemaSource::Base,
            ),
        );
        provider.insert(
            "30000",
            TypeDefinition::from_fields(83, "REAL_VALUES", "Real values", vec![field("values", FieldType::Double, 1)], SchemaSource::Base),
        );
        provider
    }

    fn append_terminator(output: &mut Vec<u8>) {
        append_i16(output, 1);
        append_positive(output, 0);
    }

    #[test]
    fn parses_fixed_and_variable_nodes_then_round_trips_exactly() {
        let mut data = header("SCH_3000000_30000", None, 0);
        append_i16(&mut data, 42);
        append_positive(&mut data, 1);
        data.extend_from_slice(&[7, b'A', 1]);
        append_i16(&mut data, -32_764);
        data.extend_from_slice(&0x3042_u16.to_be_bytes());
        append_i32(&mut data, -32_764);
        append_positive(&mut data, 0);
        data.extend_from_slice(&(-3.14158e13_f64).to_be_bytes());
        for value in 1..=14 {
            data.extend_from_slice(&f64::from(value).to_be_bytes());
        }
        append_i16(&mut data, 83);
        append_i32(&mut data, 3);
        append_positive(&mut data, 2);
        for value in [10.0_f64, 20.0, 30.0] {
            data.extend_from_slice(&value.to_be_bytes());
        }
        append_terminator(&mut data);

        let parsed = parse_xb(&data, &standard_provider(), DocumentLimits::default());
        assert!(parsed.is_ok());
        if let Ok(document) = parsed {
            assert_eq!(document.schema_provider, SchemaProviderResolution::CallerSupplied);
            assert_eq!(document.nodes.len(), 2);
            assert_eq!(document.nodes[0].index, 1);
            assert_eq!(document.nodes[0].fields.len(), 12);
            assert_eq!(document.nodes[0].fields[3].values, [FieldValue::ShortInteger(None)]);
            assert_eq!(document.nodes[0].fields[5].values, [FieldValue::Integer(None)]);
            assert_eq!(document.nodes[0].fields[7].values, [FieldValue::Double(None)]);
            assert_eq!(document.nodes[1].variable_length, Some(3));
            assert_eq!(document.nodes[1].fields[0].values.len(), 3);
            assert_eq!(document.schema_coverage.base_count, 2);
            assert_eq!(write_xb(&document), data);
        }
    }

    #[test]
    fn retains_complete_embedded_schema_on_only_the_first_node() {
        let mut provider = InMemorySchemaProvider::new();
        provider.add_schema("13006");
        let mut data = header("SCH_3000000_30000_13006", Some(205), 0);
        append_i16(&mut data, 204);
        let schema_start = data.len();
        data.push(1);
        data.push(17);
        data.extend_from_slice(b"INTERSECTION_DATA");
        data.push(17);
        data.extend_from_slice(b"Intersection data");
        data.push(6);
        data.extend_from_slice(b"values");
        data.extend_from_slice(&0_u16.to_be_bytes());
        append_positive(&mut data, 1);
        data.push(1);
        data.extend_from_slice(b"f");
        data.push(1);
        let schema_end = data.len();
        append_i32(&mut data, 1);
        append_positive(&mut data, 1);
        data.extend_from_slice(&2.5_f64.to_be_bytes());
        append_i16(&mut data, 204);
        append_i32(&mut data, 0);
        append_positive(&mut data, 2);
        append_terminator(&mut data);

        let parsed = parse_xb(&data, &provider, DocumentLimits::default());
        assert!(parsed.is_ok());
        if let Ok(document) = parsed {
            assert_eq!(document.nodes.len(), 2);
            let first = document.nodes[0].first_schema.as_ref();
            assert!(first.is_some());
            assert_eq!(first.map(|schema| schema.byte_range.clone()), Some(schema_start..schema_end));
            assert_eq!(first.map(|schema| schema.raw_schema.as_slice()), Some(&data[schema_start..schema_end]));
            assert!(document.nodes[1].first_schema.is_none());
            assert_eq!(document.schema_coverage.full_count, 1);
        }
    }

    #[test]
    fn validates_termination_trailing_bytes_indices_and_limits() {
        let provider = standard_provider();

        let missing = header("SCH_3000000_30000", None, 0);
        let error = parse_xb(&missing, &provider, DocumentLimits::default()).err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::MissingTermination));

        let mut invalid_termination = header("SCH_3000000_30000", None, 0);
        append_i16(&mut invalid_termination, 1);
        append_positive(&mut invalid_termination, 2);
        let error = parse_xb(&invalid_termination, &provider, DocumentLimits::default()).err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidTermination));

        let mut trailing = header("SCH_3000000_30000", None, 0);
        append_terminator(&mut trailing);
        trailing.push(0);
        let error = parse_xb(&trailing, &provider, DocumentLimits::default()).err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::TrailingBytes));

        let mut user_fields = header("SCH_3000000_30000", None, 1);
        append_i16(&mut user_fields, 110); // Visibility is absent from the public table.
        let error = parse_xb(&user_fields, &provider, DocumentLimits::default()).err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::UnsupportedUserFields));

        let mut limited = header("SCH_3000000_30000", None, 0);
        append_i16(&mut limited, 83);
        append_i32(&mut limited, 2);
        append_positive(&mut limited, 1);
        let error = parse_xb(&limited, &provider, DocumentLimits { max_variable_elements: 1, ..DocumentLimits::default() }).err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::LimitExceeded));
    }
}
