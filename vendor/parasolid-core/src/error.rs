//! Structured parser failures with absolute source offsets.

use std::error::Error;
use std::fmt;

/// Stable categories used to map native failures to public diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// A field extended beyond the available source bytes.
    UnexpectedEof,
    /// The binary stream did not start with the required signature.
    InvalidSignature,
    /// The binary stream uses a valid representation not implemented by this reader.
    UnsupportedBinaryFormat,
    /// A length prefix was negative or otherwise impossible.
    InvalidLength,
    /// Input or a declared field exceeded a configured resource bound.
    LimitExceeded,
    /// A caller supplied an invalid resource-limit configuration.
    InvalidLimit,
    /// A field documented as ASCII contained a non-ASCII byte.
    InvalidAscii,
    /// A logical field contained a value other than zero or one.
    InvalidLogical,
    /// A compact non-negative integer used an impossible short sequence.
    InvalidPositiveInteger,
    /// A UTF-16BE field contained an invalid code-unit sequence.
    InvalidUtf16,
    /// An optional text preamble did not contain its end marker.
    MissingTextHeaderTerminator,
    /// The optional text preamble marker was not followed by a newline.
    MissingTextHeaderNewline,
    /// The length-prefixed schema identifier was not a schema key.
    InvalidSchemaKey,
    /// The serialized user-field word count was outside the specified range.
    InvalidUserFieldSize,
    /// An embedded schema violated its structural invariants.
    InvalidSchemaDefinition,
    /// A standard text schema catalog violated its grammar or declared counts.
    InvalidSchemaCatalog,
    /// A delta schema contained an unknown edit opcode.
    UnknownSchemaOpcode,
    /// An embedded field named a scalar codec outside the documented set.
    UnsupportedSchemaFieldType,
    /// The schema catalog required to interpret a node was not loaded.
    MissingBaseSchema,
    /// A standard schema did not contain the requested node type.
    MissingSchemaType,
    /// An embedded type is known to exist in the base, but its definition is unavailable.
    UnsupportedBaseSchemaType,
    /// Membership of an embedded type in the base schema has not been established.
    UnknownBaseSchemaType,
    /// A compiled built-in profile violated its declared structural invariants.
    InvalidBuiltinProfile,
    /// A built-in provider was used with a schema key outside its exact allowlist.
    UnsupportedBuiltinSchemaKey,
    /// A selected built-in profile did not cover one encountered node type.
    BuiltinProfileUncoveredType,
    /// One node type was registered more than once.
    DuplicateSchemaType,
    /// A decoded field sequence did not match its declared count.
    SchemaFieldCountMismatch,
    /// A non-termination record used an invalid node type.
    InvalidNodeType,
    /// A variable node declared a negative element count.
    InvalidVariableLength,
    /// A node used zero or another invalid index.
    InvalidNodeIndex,
    /// Two transmitted nodes used the same non-zero index.
    DuplicateNodeIndex,
    /// The stream ended before its required termination record.
    MissingTermination,
    /// The termination record did not contain index zero.
    InvalidTermination,
    /// Bytes remained after the complete termination record.
    TrailingBytes,
    /// User fields lack visibility metadata or are outside the selected profile scope.
    UnsupportedUserFields,
    /// A text transmit stream did not start with the `T` format flag.
    InvalidTextFlag,
    /// A text transmit scalar was not valid for its schema field.
    InvalidTextToken,
    /// A text character or string used an unsupported escape sequence.
    InvalidTextEscape,
    /// A text numeric value was not followed by its required space delimiter.
    InvalidTextDelimiter,
    /// A text transmit stream ended before the current field was complete.
    UnexpectedTextEof,
    /// Non-layout text remained after the complete termination record.
    TrailingText,
    /// A normalized comparison option was negative, non-finite, or zero where forbidden.
    InvalidComparisonOption,
    /// A raw document did not contain a BODY root which can be mapped.
    MissingBrepBody,
    /// A required B-Rep schema field was absent or had the wrong value shape.
    InvalidBrepField,
    /// A B-Rep pointer was null, unresolved, or targeted an incompatible entity.
    InvalidBrepReference,
    /// Linked topology violated a ring, inverse, ownership, or manifold invariant.
    InvalidBrepTopology,
    /// Attached geometry contained a null, non-finite, or structurally inconsistent parameter.
    InvalidGeometryParameter,
}

impl ErrorKind {
    /// Return the stable public diagnostic code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnexpectedEof => "binary.truncated_field",
            Self::InvalidSignature => "binary.invalid_signature",
            Self::UnsupportedBinaryFormat => "binary.unsupported_format",
            Self::InvalidLength => "binary.invalid_length",
            Self::LimitExceeded => "limits.exceeded",
            Self::InvalidLimit => "limits.invalid_configuration",
            Self::InvalidAscii => "binary.invalid_ascii",
            Self::InvalidLogical => "binary.invalid_logical",
            Self::InvalidPositiveInteger => "binary.invalid_positive_integer",
            Self::InvalidUtf16 => "binary.invalid_utf16",
            Self::MissingTextHeaderTerminator => "binary.missing_text_header_terminator",
            Self::MissingTextHeaderNewline => "binary.missing_text_header_newline",
            Self::InvalidSchemaKey => "binary.invalid_schema_key",
            Self::InvalidUserFieldSize => "binary.invalid_user_field_size",
            Self::InvalidSchemaDefinition => "schema.invalid_definition",
            Self::InvalidSchemaCatalog => "schema.invalid_catalog",
            Self::UnknownSchemaOpcode => "schema.unknown_delta_opcode",
            Self::UnsupportedSchemaFieldType => "schema.unsupported_field_type",
            Self::MissingBaseSchema => "schema.missing_base_schema",
            Self::MissingSchemaType => "schema.missing_type_definition",
            Self::UnsupportedBaseSchemaType => "schema.unsupported_base_type",
            Self::UnknownBaseSchemaType => "schema.unknown_base_type",
            Self::InvalidBuiltinProfile => "schema.invalid_builtin_profile",
            Self::UnsupportedBuiltinSchemaKey => "schema.unsupported_builtin_key",
            Self::BuiltinProfileUncoveredType => "schema.builtin_profile_uncovered_type",
            Self::DuplicateSchemaType => "schema.duplicate_type",
            Self::SchemaFieldCountMismatch => "schema.field_count_mismatch",
            Self::InvalidNodeType => "node.invalid_type",
            Self::InvalidVariableLength => "node.invalid_variable_length",
            Self::InvalidNodeIndex => "node.invalid_index",
            Self::DuplicateNodeIndex => "node.duplicate_index",
            Self::MissingTermination => "node.missing_termination",
            Self::InvalidTermination => "node.invalid_termination",
            Self::TrailingBytes => "binary.trailing_bytes",
            Self::UnsupportedUserFields => "node.unsupported_user_fields",
            Self::InvalidTextFlag => "text.invalid_flag",
            Self::InvalidTextToken => "text.invalid_token",
            Self::InvalidTextEscape => "text.invalid_escape",
            Self::InvalidTextDelimiter => "text.invalid_delimiter",
            Self::UnexpectedTextEof => "text.truncated_field",
            Self::TrailingText => "text.trailing_content",
            Self::InvalidComparisonOption => "comparison.invalid_option",
            Self::MissingBrepBody => "brep.missing_body",
            Self::InvalidBrepField => "brep.invalid_field",
            Self::InvalidBrepReference => "brep.invalid_reference",
            Self::InvalidBrepTopology => "topology.invalid_relationship",
            Self::InvalidGeometryParameter => "geometry.invalid_parameter",
        }
    }
}

/// Machine-readable values associated with one parser failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorDetails {
    /// No extra scalar values are required.
    None,
    /// Required and available byte counts for a truncated read.
    UnexpectedEof { needed: usize, remaining: usize },
    /// One named resource exceeded its configured bound.
    LimitExceeded { resource: &'static str, actual: usize, limit: usize },
    /// One signed serialized length was invalid.
    InvalidLength { field: &'static str, value: i64 },
    /// One byte was invalid for its field codec.
    InvalidByte { field: &'static str, value: u8 },
    /// One text value was invalid for its field.
    InvalidText { field: &'static str, value: String },
    /// Expected and observed item counts.
    CountMismatch { field: &'static str, expected: usize, actual: usize },
    /// Schema catalog and node type needed for one lookup.
    SchemaLookup { schema: String, node_type: u16 },
    /// Exact built-in profile selection associated with one lookup failure.
    BuiltinProfileLookup { profile_id: String, profile_revision: u32, selected_schema_key: String, requested_schema_key: String, node_type: u16 },
    /// Node type associated with a schema failure.
    NodeType { node_type: u16 },
    /// Node index associated with a framing failure.
    NodeIndex { node_index: u32 },
    /// Required semantic field on one raw node.
    BrepField { node_index: u32, field: &'static str },
    /// Typed pointer used while mapping a semantic entity.
    BrepReference { node_index: u32, field: &'static str, target_index: u32, expected_type: &'static str },
    /// Linked relationship used by one topology invariant.
    BrepInvariant { node_index: u32, relationship: &'static str },
}

/// A strict parse failure located in the original input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    kind: ErrorKind,
    offset: usize,
    message: String,
    details: ErrorDetails,
}

impl ParseError {
    pub(crate) fn new(kind: ErrorKind, offset: usize, message: impl Into<String>, details: ErrorDetails) -> Self {
        Self { kind, offset, message: message.into(), details }
    }

    pub(crate) fn unexpected_eof(offset: usize, needed: usize, remaining: usize) -> Self {
        Self::new(
            ErrorKind::UnexpectedEof,
            offset,
            format!("field requires {needed} bytes but only {remaining} remain"),
            ErrorDetails::UnexpectedEof { needed, remaining },
        )
    }

    pub(crate) fn limit(offset: usize, resource: &'static str, actual: usize, limit: usize) -> Self {
        Self::new(
            ErrorKind::LimitExceeded,
            offset,
            format!("{resource} exceeds the configured limit"),
            ErrorDetails::LimitExceeded { resource, actual, limit },
        )
    }

    pub(crate) fn invalid_limit(resource: &'static str, value: usize) -> Self {
        Self::new(
            ErrorKind::InvalidLimit,
            0,
            format!("{resource} must be a positive integer"),
            ErrorDetails::LimitExceeded { resource, actual: value, limit: 1 },
        )
    }

    pub(crate) fn invalid_length(offset: usize, field: &'static str, value: i64) -> Self {
        Self::new(ErrorKind::InvalidLength, offset, format!("{field} has invalid length {value}"), ErrorDetails::InvalidLength { field, value })
    }

    pub(crate) fn invalid_byte(kind: ErrorKind, offset: usize, field: &'static str, value: u8) -> Self {
        Self::new(kind, offset, format!("{field} contains invalid byte 0x{value:02x}"), ErrorDetails::InvalidByte { field, value })
    }

    pub(crate) fn unexpected_text_eof(offset: usize, field: &'static str) -> Self {
        Self::new(
            ErrorKind::UnexpectedTextEof,
            offset,
            format!("text stream ended while reading {field}"),
            ErrorDetails::UnexpectedEof { needed: 1, remaining: 0 },
        )
    }

    /// Return the stable error category.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Return the absolute byte offset where validation failed.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Return the human-readable failure message without the code or offset.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Return machine-readable context for reports and language bindings.
    #[must_use]
    pub const fn details(&self) -> &ErrorDetails {
        &self.details
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} (byte_offset={}): {}", self.kind.code(), self.offset, self.message)
    }
}

impl Error for ParseError {}
