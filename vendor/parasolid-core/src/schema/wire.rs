//! Meaning-neutral reader classes used while validating inferred profiles.

use super::FieldType;

/// The parser operation needed to consume one scalar schema value.
///
/// This deliberately does not convert back into [`FieldType`]: equal reader
/// behavior is weaker evidence than equal Parasolid semantics. In particular,
/// `d`/`t` and `v`/`h` share a class while retaining distinct field types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WireDecodeClass {
    UnsignedByte,
    Character,
    Logical,
    NullableI16,
    UnicodeU16,
    NullableI32,
    PositiveInteger,
    NullableF64 { component_count: u8 },
}

impl WireDecodeClass {
    /// Return the reader class for a transmitted field codec.
    ///
    /// Opaque in-memory pointers have no transmitted representation.
    pub(crate) const fn from_field_type(field_type: FieldType) -> Option<Self> {
        match field_type {
            FieldType::UnsignedByte => Some(Self::UnsignedByte),
            FieldType::Character => Some(Self::Character),
            FieldType::Logical => Some(Self::Logical),
            FieldType::ShortInteger => Some(Self::NullableI16),
            FieldType::UnicodeCharacter => Some(Self::UnicodeU16),
            FieldType::Integer | FieldType::Tag => Some(Self::NullableI32),
            FieldType::PointerIndex => Some(Self::PositiveInteger),
            FieldType::OpaquePointer => None,
            FieldType::Double => Some(Self::NullableF64 { component_count: 1 }),
            FieldType::Interval => Some(Self::NullableF64 { component_count: 2 }),
            FieldType::Vector | FieldType::IntersectionPoint => Some(Self::NullableF64 { component_count: 3 }),
            FieldType::Box3 => Some(Self::NullableF64 { component_count: 6 }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_only_equal_reader_behavior() {
        assert_ne!(FieldType::Integer, FieldType::Tag);
        assert_eq!(WireDecodeClass::from_field_type(FieldType::Integer), WireDecodeClass::from_field_type(FieldType::Tag));
        assert_eq!(WireDecodeClass::from_field_type(FieldType::Vector), WireDecodeClass::from_field_type(FieldType::IntersectionPoint));
        assert_ne!(WireDecodeClass::from_field_type(FieldType::ShortInteger), WireDecodeClass::from_field_type(FieldType::UnicodeCharacter));
    }

    #[test]
    fn rejects_the_non_transmitted_opaque_pointer_codec() {
        assert_eq!(WireDecodeClass::from_field_type(FieldType::OpaquePointer), None);
    }
}
