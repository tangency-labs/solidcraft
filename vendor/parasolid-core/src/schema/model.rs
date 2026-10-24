//! Schema keys and resolved field/type definitions.

use std::ops::Range;

use crate::{ErrorDetails, ErrorKind, ParseError};

/// Parsed components of one `SCH_...` transmit schema key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SchemaKey {
    raw: String,
    modeller: String,
    effective: String,
    base: Option<String>,
}

impl SchemaKey {
    /// Parse a schema key and report an invalid key at byte zero.
    ///
    /// # Errors
    ///
    /// Returns `binary.invalid_schema_key` unless the key contains two or three
    /// non-empty numeric components after `SCH_`.
    pub fn parse(value: &str) -> Result<Self, ParseError> {
        Self::parse_at(value, 0)
    }

    pub(crate) fn parse_at(value: &str, offset: usize) -> Result<Self, ParseError> {
        let invalid = || {
            ParseError::new(
                ErrorKind::InvalidSchemaKey,
                offset,
                "schema key must be SCH_<modeller>_<schema> with an optional numeric base schema",
                ErrorDetails::InvalidText { field: "schema_key", value: value.to_owned() },
            )
        };
        let mut components = value.strip_prefix("SCH_").ok_or_else(invalid)?.split('_');
        let modeller = components.next().ok_or_else(invalid)?;
        let effective = components.next().ok_or_else(invalid)?;
        if !is_numeric_component(modeller) || !is_numeric_component(effective) {
            return Err(invalid());
        }
        let base = components.next();
        if components.next().is_some() || base.is_some_and(|item| !is_numeric_component(item)) {
            return Err(invalid());
        }

        Ok(Self { raw: value.to_owned(), modeller: modeller.to_owned(), effective: effective.to_owned(), base: base.map(str::to_owned) })
    }

    /// Return the original key.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Return the modeller-version component.
    #[must_use]
    pub fn modeller(&self) -> &str {
        &self.modeller
    }

    /// Return the effective-schema component.
    #[must_use]
    pub fn effective(&self) -> &str {
        &self.effective
    }

    /// Return the embedded base-schema component, when present.
    #[must_use]
    pub fn base(&self) -> Option<&str> {
        self.base.as_deref()
    }

    /// Return the schema which must be supplied by a provider.
    #[must_use]
    pub fn provider_schema(&self) -> &str {
        self.base.as_deref().unwrap_or(&self.effective)
    }
}

fn is_numeric_component(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Scalar codec named by an effective schema field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldType {
    /// `u`: unsigned byte.
    UnsignedByte,
    /// `c`: character.
    Character,
    /// `l`: logical byte.
    Logical,
    /// `n`: short integer.
    ShortInteger,
    /// `w`: Unicode code unit.
    UnicodeCharacter,
    /// `d`: signed integer.
    Integer,
    /// `p`: pointer/index.
    PointerIndex,
    /// `q`: opaque in-memory pointer used by non-transmitted schema data.
    OpaquePointer,
    /// `t`: integer tag.
    Tag,
    /// `f`: IEEE binary64.
    Double,
    /// `i`: interval.
    Interval,
    /// `v`: three-component vector.
    Vector,
    /// `b`: three intervals.
    Box3,
    /// `h`: transmitted intersection point.
    IntersectionPoint,
}

impl FieldType {
    /// Decode the one-character schema type code.
    #[must_use]
    pub fn from_code(value: &str) -> Option<Self> {
        match value {
            "u" => Some(Self::UnsignedByte),
            "c" => Some(Self::Character),
            "l" => Some(Self::Logical),
            "n" => Some(Self::ShortInteger),
            "w" => Some(Self::UnicodeCharacter),
            "d" => Some(Self::Integer),
            "p" => Some(Self::PointerIndex),
            "q" => Some(Self::OpaquePointer),
            "t" => Some(Self::Tag),
            "f" => Some(Self::Double),
            "i" => Some(Self::Interval),
            "v" => Some(Self::Vector),
            "b" => Some(Self::Box3),
            "h" => Some(Self::IntersectionPoint),
            _ => None,
        }
    }

    /// Return the stable schema type code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsignedByte => "u",
            Self::Character => "c",
            Self::Logical => "l",
            Self::ShortInteger => "n",
            Self::UnicodeCharacter => "w",
            Self::Integer => "d",
            Self::PointerIndex => "p",
            Self::OpaquePointer => "q",
            Self::Tag => "t",
            Self::Double => "f",
            Self::Interval => "i",
            Self::Vector => "v",
            Self::Box3 => "b",
            Self::IntersectionPoint => "h",
        }
    }
}

/// One effective field used to decode a transmitted node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDefinition {
    /// Field name retained from the schema.
    pub name: String,
    /// Scalar codec, or pointer/index when `pointer_class` is non-zero.
    pub field_type: FieldType,
    /// Allowed pointer node class; zero for non-pointer fields.
    pub pointer_class: u16,
    /// Zero for a scalar, one for a variable field, or a fixed array length.
    pub element_count: u32,
    /// Whether a variable field is transmitted. Fixed effective fields are true.
    pub transmitted: bool,
}

/// Provenance of an effective node-type definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemaSource {
    /// Directly supplied by the selected standard/base schema.
    Base,
    /// First-occurrence marker `0xff` reused an unchanged base definition.
    EmbeddedUnchanged,
    /// A `C/D/I/A/Z` edit sequence modified the base definition.
    EmbeddedDelta,
    /// The file supplied a complete definition for a type absent from the base.
    EmbeddedFull,
}

impl SchemaSource {
    /// Return the stable name used by reports and bindings.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::EmbeddedUnchanged => "embedded_unchanged",
            Self::EmbeddedDelta => "embedded_delta",
            Self::EmbeddedFull => "embedded_full",
        }
    }

    /// Parse a stable source name.
    #[must_use]
    pub fn from_name(value: &str) -> Option<Self> {
        match value {
            "base" => Some(Self::Base),
            "embedded_unchanged" => Some(Self::EmbeddedUnchanged),
            "embedded_delta" => Some(Self::EmbeddedDelta),
            "embedded_full" => Some(Self::EmbeddedFull),
            _ => None,
        }
    }
}

/// Effective field sequence for one node type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDefinition {
    /// Numeric node type.
    pub node_type: u16,
    /// Schema node name.
    pub name: String,
    /// Human-readable schema description.
    pub description: String,
    /// Whether the last field has a per-node variable element count.
    pub variable: bool,
    /// Effective transmitted fields in source order.
    pub fields: Vec<FieldDefinition>,
    /// Definition provenance.
    pub source: SchemaSource,
}

impl TypeDefinition {
    /// Create a definition and derive its variable-length flag.
    #[must_use]
    pub fn from_fields(
        node_type: u16,
        name: impl Into<String>,
        description: impl Into<String>,
        fields: Vec<FieldDefinition>,
        source: SchemaSource,
    ) -> Self {
        let variable = fields.last().is_some_and(|field| field.element_count == 1);
        Self { node_type, name: name.into(), description: description.into(), variable, fields, source }
    }
}

/// One decoded delta-schema instruction and its absolute source offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaEdit {
    /// Copy the next base field.
    Copy { offset: usize },
    /// Delete the next base field.
    Delete { offset: usize },
    /// Insert a field before the current base field.
    Insert { offset: usize, field: FieldDefinition },
    /// Append a field after base fields are exhausted.
    Append { offset: usize, field: FieldDefinition },
    /// End the edit sequence.
    End { offset: usize },
}

impl SchemaEdit {
    /// Return the serialized opcode.
    #[must_use]
    pub const fn opcode(&self) -> u8 {
        match self {
            Self::Copy { .. } => b'C',
            Self::Delete { .. } => b'D',
            Self::Insert { .. } => b'I',
            Self::Append { .. } => b'A',
            Self::End { .. } => b'Z',
        }
    }

    /// Return the absolute opcode offset.
    #[must_use]
    pub const fn offset(&self) -> usize {
        match self {
            Self::Copy { offset } | Self::Delete { offset } | Self::Insert { offset, .. } | Self::Append { offset, .. } | Self::End { offset } => {
                *offset
            }
        }
    }

    /// Return the field carried by an insert or append instruction.
    #[must_use]
    pub const fn field(&self) -> Option<&FieldDefinition> {
        match self {
            Self::Insert { field, .. } | Self::Append { field, .. } => Some(field),
            Self::Copy { .. } | Self::Delete { .. } | Self::End { .. } => None,
        }
    }
}

/// One first-occurrence schema blob and its resolved effective definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaResolution {
    /// Effective node-type definition.
    pub definition: TypeDefinition,
    /// Exact embedded bytes consumed from the file.
    pub raw_schema: Vec<u8>,
    /// Half-open absolute range of `raw_schema` in the input.
    pub byte_range: Range<usize>,
    /// Parsed edit sequence; empty for full and unchanged schemas.
    pub edits: Vec<SchemaEdit>,
}

impl SchemaResolution {
    /// Return the number of consumed schema bytes.
    #[must_use]
    pub fn consumed(&self) -> usize {
        self.byte_range.end - self.byte_range.start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_and_embedded_schema_keys() {
        let standard = SchemaKey::parse("SCH_3700000_36001");
        assert!(standard.is_ok());
        if let Ok(key) = standard {
            assert_eq!(key.modeller(), "3700000");
            assert_eq!(key.effective(), "36001");
            assert_eq!(key.base(), None);
            assert_eq!(key.provider_schema(), "36001");
        }

        let embedded = SchemaKey::parse("SCH_3000310_30000_13006");
        assert!(embedded.is_ok());
        if let Ok(key) = embedded {
            assert_eq!(key.base(), Some("13006"));
            assert_eq!(key.provider_schema(), "13006");
        }
    }

    #[test]
    fn rejects_invalid_schema_key_components() {
        for value in ["SCH_", "SCH_bad_13006", "SCH_1_2_3_4", "OTHER_1_2"] {
            let error = SchemaKey::parse(value).err();
            assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidSchemaKey));
        }
    }

    #[test]
    fn maps_every_documented_field_type_code() {
        for (code, expected) in [
            ("u", FieldType::UnsignedByte),
            ("c", FieldType::Character),
            ("l", FieldType::Logical),
            ("n", FieldType::ShortInteger),
            ("w", FieldType::UnicodeCharacter),
            ("d", FieldType::Integer),
            ("p", FieldType::PointerIndex),
            ("q", FieldType::OpaquePointer),
            ("t", FieldType::Tag),
            ("f", FieldType::Double),
            ("i", FieldType::Interval),
            ("v", FieldType::Vector),
            ("b", FieldType::Box3),
            ("h", FieldType::IntersectionPoint),
        ] {
            assert_eq!(FieldType::from_code(code), Some(expected));
            assert_eq!(expected.code(), code);
        }
        assert_eq!(FieldType::from_code("unknown"), None);
    }
}
