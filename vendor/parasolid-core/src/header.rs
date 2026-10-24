//! Strict level-zero inspection of an `X_B` transmit header.

use std::ops::Range;

use crate::{BinaryReader, ErrorDetails, ErrorKind, ParseError, SchemaKey};

const TEXT_HEADER_END: &[u8] = b"**END_OF_HEADER**";

/// Resource bounds needed before node-stream parsing begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InspectionLimits {
    /// Maximum accepted source size in bytes.
    pub max_file_size: usize,
    /// Maximum accepted length of one header string or text preamble.
    pub max_string_bytes: usize,
}

impl Default for InspectionLimits {
    fn default() -> Self {
        Self { max_file_size: 256 * 1024 * 1024, max_string_bytes: 16 * 1024 * 1024 }
    }
}

/// Physical binary representation selected by the `PS` flag sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XbBinaryFormat {
    /// Big-endian integers, IEEE binary64 values, and ASCII characters.
    Neutral,
}

impl XbBinaryFormat {
    /// Stable name exposed by language bindings and reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
        }
    }
}

/// Header fields confirmed without decoding a node body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XbHeader {
    /// Leading `PS` signature bytes.
    pub signature: [u8; 2],
    /// Physical representation encoded by the complete four-byte flag sequence.
    pub binary_format: XbBinaryFormat,
    /// Length-prefixed modeller description.
    pub modeller_version: String,
    /// Length-prefixed `SCH_...` identifier.
    pub schema_key: String,
    /// User-field length in integer words, specified as a value from zero to 16.
    pub user_field_size: u8,
    /// Maximum node type present only when the key names an embedded base schema.
    pub schema_max_type: Option<u16>,
    /// Complete input size.
    pub file_size: usize,
    /// Optional leading text-preamble range, including its newline.
    pub text_header_range: Option<Range<usize>>,
    /// Binary `PS` header range, ending at the first node-stream byte.
    pub binary_header_range: Range<usize>,
    /// Complete leading header range, including any text preamble.
    pub header_range: Range<usize>,
}

/// Inspect one raw `X_B` family payload without decoding its node stream.
///
/// # Errors
///
/// Returns a structured error for invalid limits, oversized input, truncated
/// framing, non-ASCII strings, an unsupported binary representation, an invalid
/// signature or schema key, or an invalid user-field size.
pub fn inspect_xb(data: &[u8], limits: InspectionLimits) -> Result<XbHeader, ParseError> {
    validate_limits(limits)?;
    if data.len() > limits.max_file_size {
        return Err(ParseError::limit(0, "file_size", data.len(), limits.max_file_size));
    }

    let (binary_start, text_header_range) = locate_payload_start(data, limits.max_string_bytes)?;
    let mut reader = BinaryReader::with_position(data, binary_start)?;
    let signature_offset = reader.position();
    let signature_bytes = reader.bytes(2)?;
    if signature_bytes != b"PS" {
        return Err(ParseError::new(ErrorKind::InvalidSignature, signature_offset, "expected X_B binary signature 'PS'", ErrorDetails::None));
    }

    let format_offset = reader.position();
    let format_bytes = reader.bytes(2)?;
    let binary_format = if format_bytes == b"\0\0" {
        XbBinaryFormat::Neutral
    } else {
        let invalid_index = format_bytes.iter().position(|value| *value != 0).unwrap_or(0);
        return Err(ParseError::invalid_byte(
            ErrorKind::UnsupportedBinaryFormat,
            format_offset + invalid_index,
            "PS binary format flag",
            format_bytes[invalid_index],
        ));
    };

    let modeller_version = reader.ascii_u16_length(limits.max_string_bytes)?;
    let schema_length_offset = reader.position();
    let schema_key = reader.ascii_i32_length(limits.max_string_bytes, "schema_key")?;
    let parsed_schema_key = SchemaKey::parse_at(&schema_key, schema_length_offset + 4)?;
    let has_embedded_schema = parsed_schema_key.base().is_some();

    let schema_max_type = has_embedded_schema.then(|| reader.u16()).transpose()?;
    let user_field_offset = reader.position();
    let serialized_user_field_size = reader.i32()?;
    let Ok(user_field_size) = u8::try_from(serialized_user_field_size) else {
        return Err(ParseError::new(
            ErrorKind::InvalidUserFieldSize,
            user_field_offset,
            "user field size must be between 0 and 16 integer words",
            ErrorDetails::InvalidLength { field: "user_field_size", value: i64::from(serialized_user_field_size) },
        ));
    };
    if user_field_size > 16 {
        return Err(ParseError::new(
            ErrorKind::InvalidUserFieldSize,
            user_field_offset,
            "user field size must be between 0 and 16 integer words",
            ErrorDetails::InvalidLength { field: "user_field_size", value: i64::from(serialized_user_field_size) },
        ));
    }
    let body_offset = reader.position();
    Ok(XbHeader {
        signature: [signature_bytes[0], signature_bytes[1]],
        binary_format,
        modeller_version,
        schema_key,
        user_field_size,
        schema_max_type,
        file_size: data.len(),
        text_header_range,
        binary_header_range: binary_start..body_offset,
        header_range: 0..body_offset,
    })
}

fn validate_limits(limits: InspectionLimits) -> Result<(), ParseError> {
    if limits.max_file_size == 0 {
        return Err(ParseError::invalid_limit("max_file_size", 0));
    }
    if limits.max_string_bytes == 0 {
        return Err(ParseError::invalid_limit("max_string_bytes", 0));
    }
    Ok(())
}

pub(crate) fn locate_payload_start(data: &[u8], max_text_header_bytes: usize) -> Result<(usize, Option<Range<usize>>), ParseError> {
    if !data.starts_with(b"**") {
        return Ok((0, None));
    }

    let search_length = data.len().min(max_text_header_bytes);
    let marker = data[..search_length].windows(TEXT_HEADER_END.len()).position(|window| window == TEXT_HEADER_END);
    let Some(marker_start) = marker else {
        if data.len() > max_text_header_bytes {
            return Err(ParseError::limit(0, "text_header_bytes", data.len(), max_text_header_bytes));
        }
        return Err(ParseError::new(ErrorKind::MissingTextHeaderTerminator, 0, "text preamble is missing '**END_OF_HEADER**'", ErrorDetails::None));
    };

    let after_marker = marker_start + TEXT_HEADER_END.len();
    let mut line_end = after_marker;
    while line_end < search_length && data[line_end] == b'*' {
        line_end += 1;
    }
    if line_end == search_length && line_end < data.len() {
        return Err(ParseError::limit(0, "text_header_bytes", line_end.saturating_add(1), max_text_header_bytes));
    }

    let newline_length = if data.get(line_end..line_end.saturating_add(2)) == Some(b"\r\n") {
        2
    } else if data.get(line_end) == Some(&b'\n') {
        1
    } else {
        return Err(ParseError::new(
            ErrorKind::MissingTextHeaderNewline,
            line_end,
            "text preamble terminator line must end with LF or CRLF",
            ErrorDetails::None,
        ));
    };
    let binary_start = line_end + newline_length;
    if binary_start > max_text_header_bytes {
        return Err(ParseError::limit(0, "text_header_bytes", binary_start, max_text_header_bytes));
    }
    Ok((binary_start, Some(0..binary_start)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn append_u16_ascii(output: &mut Vec<u8>, value: &str) {
        let length = u16::try_from(value.len());
        assert!(length.is_ok());
        if let Ok(length) = length {
            output.extend_from_slice(&length.to_be_bytes());
            output.extend_from_slice(value.as_bytes());
        }
    }

    fn append_i32_ascii(output: &mut Vec<u8>, value: &str) {
        let length = i32::try_from(value.len());
        assert!(length.is_ok());
        if let Ok(length) = length {
            output.extend_from_slice(&length.to_be_bytes());
            output.extend_from_slice(value.as_bytes());
        }
    }

    fn fixture(schema_key: &str) -> Vec<u8> {
        let mut output = b"PS\0\0".to_vec();
        append_u16_ascii(&mut output, ": TRANSMIT FILE created by modeller version 3000000");
        append_i32_ascii(&mut output, schema_key);
        if schema_key.split('_').count() == 4 {
            output.extend_from_slice(&205_u16.to_be_bytes());
        }
        output.extend_from_slice(&0_i32.to_be_bytes());
        output
    }

    #[test]
    fn inspects_direct_binary_header() {
        let data = fixture("SCH_3000310_30000_13006");
        let result = inspect_xb(&data, InspectionLimits::default());

        assert!(result.is_ok());
        if let Ok(header) = result {
            assert_eq!(header.signature, *b"PS");
            assert_eq!(header.binary_format, XbBinaryFormat::Neutral);
            assert_eq!(header.schema_key, "SCH_3000310_30000_13006");
            assert_eq!(header.user_field_size, 0);
            assert_eq!(header.schema_max_type, Some(205));
            assert_eq!(header.binary_header_range, 0..data.len());
            assert_eq!(header.header_range, 0..data.len());
            assert_eq!(header.text_header_range, None);
        }
    }

    #[test]
    fn standard_schema_header_ends_before_first_node_type() {
        let mut data = fixture("SCH_3000000_30000");
        let binary_header_end = data.len();
        data.extend_from_slice(&12_i16.to_be_bytes());

        let result = inspect_xb(&data, InspectionLimits::default());
        assert!(result.is_ok());
        if let Ok(header) = result {
            assert_eq!(header.schema_max_type, None);
            assert_eq!(header.binary_header_range, 0..binary_header_end);
            assert_eq!(&data[header.binary_header_range.end..], &12_i16.to_be_bytes());
        }
    }

    #[test]
    fn inspects_lf_and_crlf_text_preambles() {
        for newline in [b"\n".as_slice(), b"\r\n".as_slice()] {
            for padding_length in [0, 61] {
                let mut data = b"**ABCDEFGHIJKLMNOPQRSTUVWXYZ\n**END_OF_HEADER**".to_vec();
                data.resize(data.len() + padding_length, b'*');
                data.extend_from_slice(newline);
                let binary_start = data.len();
                data.extend_from_slice(&fixture("SCH_2601246_26105_13006"));

                let result = inspect_xb(&data, InspectionLimits::default());
                assert!(result.is_ok());
                if let Ok(header) = result {
                    assert_eq!(header.text_header_range, Some(0..binary_start));
                    assert_eq!(header.binary_header_range.start, binary_start);
                }
            }
        }
    }

    #[test]
    fn rejects_non_padding_bytes_before_text_header_newline() {
        let data = b"**header\n**END_OF_HEADER**x\nPS";
        let expected_offset = b"**header\n**END_OF_HEADER**".len();
        let error = inspect_xb(data, InspectionLimits::default()).err();

        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::MissingTextHeaderNewline));
        assert_eq!(error.as_ref().map(ParseError::offset), Some(expected_offset));
    }

    #[test]
    fn every_truncated_header_prefix_fails_without_guessing() {
        let data = fixture("SCH_3700000_37000_13006");
        for cut in 0..data.len() {
            let result = inspect_xb(&data[..cut], InspectionLimits::default());
            assert!(result.is_err(), "prefix ending at {cut} unexpectedly parsed");
        }
    }

    #[test]
    fn distinguishes_synthetic_v26_v30_and_v37_schema_keys() {
        for schema_key in ["SCH_2601246_26105_13006", "SCH_3000310_30000_13006", "SCH_3700000_37000_13006"] {
            let data = fixture(schema_key);
            let result = inspect_xb(&data, InspectionLimits::default());
            assert_eq!(result.map(|header| header.schema_key), Ok(schema_key.to_owned()));
        }
    }

    #[test]
    fn reports_invalid_signature_at_binary_start() {
        let error = inspect_xb(b"NO", InspectionLimits::default()).err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidSignature));
        assert_eq!(error.as_ref().map(ParseError::offset), Some(0));
    }

    #[test]
    fn rejects_typed_binary_at_format_flag_offset() {
        let error = inspect_xb(b"PS\0\x01\0\0\0", InspectionLimits::default()).err();

        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::UnsupportedBinaryFormat));
        assert_eq!(error.as_ref().map(ParseError::offset), Some(3));
    }

    #[test]
    fn rejects_negative_schema_length_at_prefix_offset() {
        let modeller = ": TRANSMIT FILE created by modeller version 3000000";
        let mut data = b"PS\0\0".to_vec();
        append_u16_ascii(&mut data, modeller);
        let length_offset = data.len();
        data.extend_from_slice(&(-1_i32).to_be_bytes());
        let error = inspect_xb(&data, InspectionLimits::default()).err();

        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidLength));
        assert_eq!(error.as_ref().map(ParseError::offset), Some(length_offset));
    }

    #[test]
    fn applies_file_and_string_bounds() {
        let data = fixture("SCH_3000310_30000_13006");
        let file_error = inspect_xb(&data, InspectionLimits { max_file_size: data.len() - 1, max_string_bytes: 1024 }).err();
        assert_eq!(file_error.as_ref().map(ParseError::kind), Some(ErrorKind::LimitExceeded));

        let string_error = inspect_xb(&data, InspectionLimits { max_file_size: data.len(), max_string_bytes: 8 }).err();
        assert_eq!(string_error.as_ref().map(ParseError::kind), Some(ErrorKind::LimitExceeded));
        assert_eq!(string_error.as_ref().map(ParseError::offset), Some(4));
    }

    #[test]
    fn rejects_missing_text_header_terminator_and_newline() {
        let missing = inspect_xb(b"**not-complete", InspectionLimits::default()).err();
        assert_eq!(missing.as_ref().map(ParseError::kind), Some(ErrorKind::MissingTextHeaderTerminator));

        let no_newline = inspect_xb(b"**header**END_OF_HEADER**PS", InspectionLimits::default()).err();
        assert_eq!(no_newline.as_ref().map(ParseError::kind), Some(ErrorKind::MissingTextHeaderNewline));
    }

    #[test]
    fn rejects_invalid_user_field_size() {
        let mut data = fixture("SCH_3000310_30000_13006");
        let user_field_offset = data.len() - 4;
        data[user_field_offset..user_field_offset + 4].copy_from_slice(&17_i32.to_be_bytes());

        let error = inspect_xb(&data, InspectionLimits::default()).err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidUserFieldSize));
        assert_eq!(error.as_ref().map(ParseError::offset), Some(user_field_offset));
    }

    #[test]
    fn rejects_schema_keys_with_non_numeric_or_extra_components() {
        for schema_key in ["SCH_bad_30000", "SCH_3000000_30000_13006_extra"] {
            let data = fixture(schema_key);
            let error = inspect_xb(&data, InspectionLimits::default()).err();
            assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidSchemaKey));
        }
    }
}
