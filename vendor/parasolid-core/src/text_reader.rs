//! Schema-driven primitive reader for the compact Parasolid `X_T` stream.

use crate::{ErrorDetails, ErrorKind, ParseError};

const MAX_COMPACT_INDEX: u64 = 1_073_709_055;

#[derive(Debug, Clone)]
struct CharacterExpansion {
    remaining_spaces: u8,
}

/// Logical text stream with physical newlines removed and source offsets retained.
#[derive(Debug, Clone)]
pub(crate) struct TextReader {
    logical: Vec<u8>,
    source_offsets: Vec<usize>,
    source_len: usize,
    position: usize,
    character_expansion: Option<CharacterExpansion>,
}

impl TextReader {
    /// Create a reader at the first internal `T` flag after the common header.
    pub(crate) fn new(data: &[u8], start: usize) -> Result<Self, ParseError> {
        if start > data.len() {
            return Err(ParseError::unexpected_text_eof(data.len(), "text payload"));
        }
        let mut logical = Vec::with_capacity(data.len().saturating_sub(start));
        let mut source_offsets = Vec::with_capacity(data.len().saturating_sub(start));
        let mut line_start = start;
        while line_start < data.len() {
            let line_end =
                data[line_start..].iter().position(|byte| matches!(byte, b'\r' | b'\n')).map_or(data.len(), |relative| line_start + relative);
            let mut content_end = line_end;
            while content_end > line_start && data[content_end - 1] == b' ' {
                content_end -= 1;
            }
            for (offset, byte) in data[line_start..content_end].iter().copied().enumerate() {
                // Numeric and structural readers validate their own ASCII grammar. Keep
                // non-ASCII bytes and literal tabs here so schema-driven character
                // arrays preserve text emitted by real exporters. A tab is data,
                // never layout: numeric and structural readers reject it below.
                if byte.is_ascii_control() && byte != b'\t' {
                    return Err(ParseError::invalid_byte(ErrorKind::InvalidAscii, line_start + offset, "text_stream", byte));
                }
                logical.push(byte);
                source_offsets.push(line_start + offset);
            }
            if line_end == data.len() {
                break;
            }
            line_start = line_end + 1;
            if data[line_end] == b'\r' && data.get(line_start) == Some(&b'\n') {
                line_start += 1;
            }
        }
        Ok(Self { logical, source_offsets, source_len: data.len(), position: 0, character_expansion: None })
    }

    /// Return the absolute byte offset of the next logical character.
    pub(crate) fn source_position(&self) -> usize {
        self.source_offsets.get(self.position).copied().unwrap_or(self.source_len)
    }

    /// Return whether all non-newline source characters have been consumed.
    pub(crate) fn is_empty(&self) -> bool {
        self.position == self.logical.len() && self.character_expansion.is_none()
    }

    /// Return the number of remaining logical source characters.
    pub(crate) fn remaining(&self) -> usize {
        self.logical.len().saturating_sub(self.position)
    }

    /// Return the next physical logical byte without consuming it.
    pub(crate) fn peek(&self) -> Option<u8> {
        self.logical.get(self.position).copied()
    }

    /// Read one exact physical character such as the `T` flag or a schema opcode.
    pub(crate) fn raw_character(&mut self, field: &'static str) -> Result<u8, ParseError> {
        if self.peek() == Some(b'\t') {
            return Err(ParseError::invalid_byte(ErrorKind::InvalidAscii, self.source_position(), field, b'\t'));
        }
        if self.character_expansion.is_some() {
            return Err(ParseError::new(
                ErrorKind::InvalidTextEscape,
                self.source_position(),
                "a compressed character sequence crossed a structural field boundary",
                ErrorDetails::InvalidText { field, value: "\\9".to_owned() },
            ));
        }
        self.take(field)
    }

    /// Require one exact physical character.
    pub(crate) fn expect(&mut self, expected: u8, field: &'static str, kind: ErrorKind) -> Result<(), ParseError> {
        let offset = self.source_position();
        let observed = self.raw_character(field)?;
        if observed != expected {
            return Err(ParseError::invalid_byte(kind, offset, field, observed));
        }
        Ok(())
    }

    /// Decode one `c` value, including the post-V12 escape sequences.
    pub(crate) fn character(&mut self) -> Result<u8, ParseError> {
        if let Some(expansion) = &mut self.character_expansion {
            expansion.remaining_spaces -= 1;
            let value = b' ';
            if expansion.remaining_spaces == 0 {
                self.character_expansion = None;
            }
            return Ok(value);
        }

        let escape_offset = self.source_position();
        let value = self.take("character")?;
        if value != b'\\' {
            return Ok(value);
        }
        let escaped = self.take("character_escape")?;
        match escaped {
            b'0' => Ok(0),
            b'n' => Ok(b'\r'),
            b'r' => Ok(b'\n'),
            b'\\' => Ok(b'\\'),
            b'9' => {
                self.character_expansion = Some(CharacterExpansion { remaining_spaces: 8 });
                Ok(b' ')
            }
            _ => Err(ParseError::invalid_byte(ErrorKind::InvalidTextEscape, escape_offset, "character_escape", escaped)),
        }
    }

    /// Read one `l` value; logicals have no trailing separator.
    pub(crate) fn logical(&mut self) -> Result<bool, ParseError> {
        let offset = self.source_position();
        match self.raw_character("logical")? {
            b'F' => Ok(false),
            b'T' => Ok(true),
            value => Err(ParseError::invalid_byte(ErrorKind::InvalidLogical, offset, "logical", value)),
        }
    }

    /// Read a byte-sized integer written as decimal text.
    pub(crate) fn unsigned_byte(&mut self, field: &'static str) -> Result<u8, ParseError> {
        let offset = self.source_position();
        let value = self.unsigned(field)?;
        u8::try_from(value).map_err(|_| Self::out_of_range(offset, field, value))
    }

    /// Read an unsigned short written as decimal text.
    pub(crate) fn unsigned_short(&mut self, field: &'static str) -> Result<u16, ParseError> {
        let offset = self.source_position();
        let value = self.unsigned(field)?;
        u16::try_from(value).map_err(|_| Self::out_of_range(offset, field, value))
    }

    /// Read one 16-bit Unicode code unit written as signed or unsigned decimal text.
    pub(crate) fn unicode_character(&mut self, field: &'static str) -> Result<u16, ParseError> {
        let offset = self.source_position();
        let value = self.signed(field)?;
        if let Ok(unsigned) = u16::try_from(value) {
            return Ok(unsigned);
        }
        i16::try_from(value).map(|signed| u16::from_ne_bytes(signed.to_ne_bytes())).map_err(|_| ParseError::invalid_length(offset, field, value))
    }

    /// Read a non-negative integer used for indices, lengths, and array counts.
    pub(crate) fn positive_integer(&mut self, field: &'static str) -> Result<u32, ParseError> {
        self.positive_integer_with_termination(field, false)
    }

    /// Read the final termination index, for which end-of-stream is a valid delimiter.
    pub(crate) fn termination_index(&mut self, field: &'static str) -> Result<u32, ParseError> {
        self.positive_integer_with_termination(field, true)
    }

    fn positive_integer_with_termination(&mut self, field: &'static str, allow_end: bool) -> Result<u32, ParseError> {
        let offset = self.source_position();
        let (token, token_offset) = self.token_with_termination(field, allow_end)?;
        let value = std::str::from_utf8(&token)
            .ok()
            .and_then(|text| text.parse::<u64>().ok())
            .ok_or_else(|| Self::invalid_token(token_offset, field, &token))?;
        if value > MAX_COMPACT_INDEX {
            return Err(Self::out_of_range(offset, field, value));
        }
        u32::try_from(value).map_err(|_| Self::out_of_range(offset, field, value))
    }

    /// Read a non-null signed short.
    pub(crate) fn short(&mut self, field: &'static str) -> Result<i16, ParseError> {
        let offset = self.source_position();
        let value = self.signed(field)?;
        i16::try_from(value).map_err(|_| ParseError::invalid_length(offset, field, value))
    }

    /// Read a nullable signed short.
    pub(crate) fn nullable_short(&mut self, field: &'static str) -> Result<Option<i16>, ParseError> {
        let offset = self.source_position();
        self.nullable_signed(field)?
            .map_or(Ok(None), |value| i16::try_from(value).map(Some).map_err(|_| ParseError::invalid_length(offset, field, value)))
    }

    /// Read a nullable signed 32-bit integer.
    pub(crate) fn nullable_integer(&mut self, field: &'static str) -> Result<Option<i32>, ParseError> {
        let offset = self.source_position();
        self.nullable_signed(field)?
            .map_or(Ok(None), |value| i32::try_from(value).map(Some).map_err(|_| ParseError::invalid_length(offset, field, value)))
    }

    /// Read a non-null signed 32-bit integer.
    pub(crate) fn integer(&mut self, field: &'static str) -> Result<i32, ParseError> {
        let offset = self.source_position();
        let value = self.signed(field)?;
        i32::try_from(value).map_err(|_| ParseError::invalid_length(offset, field, value))
    }

    /// Read one nullable decimal real.
    pub(crate) fn nullable_double(&mut self, field: &'static str) -> Result<Option<f64>, ParseError> {
        if self.peek() == Some(b'?') {
            self.raw_character(field)?;
            return Ok(None);
        }
        let (token, offset) = self.token(field)?;
        let value =
            std::str::from_utf8(&token).ok().and_then(|text| text.parse::<f64>().ok()).ok_or_else(|| Self::invalid_token(offset, field, &token))?;
        Ok(Some(value))
    }

    /// Read a length-prefixed ASCII string.
    pub(crate) fn short_string(&mut self, resource: &'static str, max_string_bytes: usize) -> Result<String, ParseError> {
        let length_offset = self.source_position();
        let length = usize::from(self.unsigned_byte(resource)?);
        if length > max_string_bytes {
            return Err(ParseError::limit(length_offset, resource, length, max_string_bytes));
        }
        self.fixed_string(length, resource)
    }

    /// Read an integer-length-prefixed ASCII string such as the internal headers.
    pub(crate) fn integer_string(&mut self, resource: &'static str, max_string_bytes: usize) -> Result<String, ParseError> {
        let length_offset = self.source_position();
        let length =
            usize::try_from(self.unsigned(resource)?).map_err(|_| ParseError::limit(length_offset, resource, usize::MAX, max_string_bytes))?;
        if length > max_string_bytes {
            return Err(ParseError::limit(length_offset, resource, length, max_string_bytes));
        }
        self.fixed_string(length, resource)
    }

    fn fixed_string(&mut self, length: usize, resource: &'static str) -> Result<String, ParseError> {
        let offset = self.source_position();
        let mut output = Vec::with_capacity(length);
        for _ in 0..length {
            output.push(self.character()?);
        }
        String::from_utf8(output).map_err(|error| {
            ParseError::new(
                ErrorKind::InvalidAscii,
                offset + error.utf8_error().valid_up_to(),
                "text string contains non-ASCII data",
                ErrorDetails::InvalidText { field: resource, value: "non-ASCII".to_owned() },
            )
        })
    }

    fn unsigned(&mut self, field: &'static str) -> Result<u64, ParseError> {
        let (token, offset) = self.token(field)?;
        let value =
            std::str::from_utf8(&token).ok().and_then(|text| text.parse::<u64>().ok()).ok_or_else(|| Self::invalid_token(offset, field, &token))?;
        Ok(value)
    }

    fn signed(&mut self, field: &'static str) -> Result<i64, ParseError> {
        let (token, offset) = self.token(field)?;
        std::str::from_utf8(&token).ok().and_then(|text| text.parse::<i64>().ok()).ok_or_else(|| Self::invalid_token(offset, field, &token))
    }

    fn nullable_signed(&mut self, field: &'static str) -> Result<Option<i64>, ParseError> {
        if self.peek() == Some(b'?') {
            self.raw_character(field)?;
            return Ok(None);
        }
        let (token, offset) = self.token(field)?;
        std::str::from_utf8(&token).ok().and_then(|text| text.parse::<i64>().ok()).map(Some).ok_or_else(|| Self::invalid_token(offset, field, &token))
    }

    fn token(&mut self, field: &'static str) -> Result<(Vec<u8>, usize), ParseError> {
        self.token_with_termination(field, false)
    }

    fn token_with_termination(&mut self, field: &'static str, allow_end: bool) -> Result<(Vec<u8>, usize), ParseError> {
        if self.character_expansion.is_some() {
            return Err(ParseError::new(
                ErrorKind::InvalidTextEscape,
                self.source_position(),
                "a compressed character sequence crossed into a numeric field",
                ErrorDetails::InvalidText { field, value: "\\9".to_owned() },
            ));
        }
        let offset = self.source_position();
        let start = self.position;
        while let Some(byte) = self.peek() {
            if byte == b'\t' {
                return Err(ParseError::invalid_byte(ErrorKind::InvalidAscii, self.source_position(), field, byte));
            }
            if byte == b' ' {
                break;
            }
            self.position += 1;
        }
        if self.position == start {
            return Err(ParseError::new(
                ErrorKind::InvalidTextToken,
                offset,
                format!("{field} has an empty text token"),
                ErrorDetails::InvalidText { field, value: String::new() },
            ));
        }
        if self.peek().is_none() && !allow_end {
            return Err(ParseError::new(
                ErrorKind::InvalidTextDelimiter,
                self.source_position(),
                format!("{field} is not followed by the required space delimiter"),
                ErrorDetails::InvalidText { field, value: String::from_utf8_lossy(&self.logical[start..self.position]).into_owned() },
            ));
        }
        let token = self.logical[start..self.position].to_vec();
        if self.peek().is_some() {
            self.position += 1;
        }
        Ok((token, offset))
    }

    fn take(&mut self, field: &'static str) -> Result<u8, ParseError> {
        let Some(value) = self.peek() else {
            return Err(ParseError::unexpected_text_eof(self.source_position(), field));
        };
        self.position += 1;
        Ok(value)
    }

    fn invalid_token(offset: usize, field: &'static str, token: &[u8]) -> ParseError {
        ParseError::new(
            ErrorKind::InvalidTextToken,
            offset,
            format!("{field} contains an invalid text token"),
            ErrorDetails::InvalidText { field, value: String::from_utf8_lossy(token).into_owned() },
        )
    }

    fn out_of_range(offset: usize, field: &'static str, value: u64) -> ParseError {
        let value = i64::try_from(value).unwrap_or(i64::MAX);
        ParseError::invalid_length(offset, field, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_newlines_inside_numbers_and_retains_source_offsets() -> Result<(), ParseError> {
        let mut reader = TextReader::new(b"12\n34 5 X", 0)?;
        assert_eq!(reader.integer("first"), Ok(1234));
        assert_eq!(reader.source_position(), 6);
        assert_eq!(reader.integer("second"), Ok(5));
        assert_eq!(reader.raw_character("suffix"), Ok(b'X'));
        assert!(reader.is_empty());
        Ok(())
    }

    #[test]
    fn decodes_character_escapes_and_compacted_spaces() -> Result<(), ParseError> {
        let mut reader = TextReader::new(b"\\0\\n\\r\\\\\\9", 0)?;
        assert_eq!(reader.character(), Ok(0));
        assert_eq!(reader.character(), Ok(b'\r'));
        assert_eq!(reader.character(), Ok(b'\n'));
        assert_eq!(reader.character(), Ok(b'\\'));
        for _ in 0..9 {
            assert_eq!(reader.character(), Ok(b' '));
        }
        assert!(reader.is_empty());
        Ok(())
    }

    #[test]
    fn preserves_non_ascii_bytes_for_schema_driven_character_fields() -> Result<(), ParseError> {
        let encoded = "û".as_bytes();
        let mut reader = TextReader::new(encoded, 0)?;
        assert_eq!(reader.character(), Ok(encoded[0]));
        assert_eq!(reader.character(), Ok(encoded[1]));
        assert!(reader.is_empty());
        Ok(())
    }

    #[test]
    fn preserves_literal_tabs_in_character_data_and_source_offsets() -> Result<(), ParseError> {
        let mut reader = TextReader::new(b"A\t\n\tB\t\r\nC", 0)?;
        for (offset, expected) in [(0, b'A'), (1, b'\t'), (3, b'\t'), (4, b'B'), (5, b'\t'), (8, b'C')] {
            assert_eq!(reader.source_position(), offset);
            assert_eq!(reader.character(), Ok(expected));
        }
        assert!(reader.is_empty());
        Ok(())
    }

    #[test]
    fn rejects_tabs_in_numeric_and_structural_fields() -> Result<(), ParseError> {
        for token in [b"\t12 ".as_slice(), b"1\t2 ", b"12\t", b"12\t "] {
            let mut reader = TextReader::new(token, 0)?;
            let error = reader.integer("value").err();
            assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidAscii));
            assert_eq!(error.as_ref().map(ParseError::offset), token.iter().position(|byte| *byte == b'\t'));
        }
        let mut reader = TextReader::new(b"\t", 0)?;
        assert_eq!(reader.raw_character("schema_opcode").err().as_ref().map(ParseError::kind), Some(ErrorKind::InvalidAscii));
        let mut logical = TextReader::new(b"\t", 0)?;
        assert_eq!(logical.logical().err().as_ref().map(ParseError::kind), Some(ErrorKind::InvalidAscii));
        Ok(())
    }

    #[test]
    fn still_rejects_other_literal_control_bytes() {
        for byte in (0_u8..=31).chain(std::iter::once(127)) {
            if matches!(byte, b'\t' | b'\r' | b'\n') {
                continue;
            }
            let error = TextReader::new(&[b'A', byte], 0).err();
            assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidAscii));
            assert_eq!(error.as_ref().map(ParseError::offset), Some(1));
        }
    }

    #[test]
    fn distinguishes_logical_and_numeric_delimiter_errors() -> Result<(), ParseError> {
        let mut logical = TextReader::new(b"TFX", 0)?;
        assert_eq!(logical.logical(), Ok(true));
        assert_eq!(logical.logical(), Ok(false));
        let error = logical.logical().err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidLogical));

        let mut numeric = TextReader::new(b"12", 0)?;
        let error = numeric.integer("value").err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidTextDelimiter));
        Ok(())
    }

    #[test]
    fn decodes_signed_and_unsigned_unicode_code_units() -> Result<(), ParseError> {
        let mut reader = TextReader::new(b"-28440 37096 65535 -32768 X", 0)?;
        assert_eq!(reader.unicode_character("unicode"), Ok(0x90e8));
        assert_eq!(reader.unicode_character("unicode"), Ok(0x90e8));
        assert_eq!(reader.unicode_character("unicode"), Ok(u16::MAX));
        assert_eq!(reader.unicode_character("unicode"), Ok(0x8000));
        assert_eq!(reader.raw_character("suffix"), Ok(b'X'));
        assert!(reader.is_empty());

        for token in [b"-32769 X".as_slice(), b"65536 X".as_slice()] {
            let mut invalid = TextReader::new(token, 0)?;
            let error = invalid.unicode_character("unicode").err();
            assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidLength));
        }
        Ok(())
    }
}
