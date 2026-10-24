//! Bounds-checked big-endian primitive reader.

use crate::{ErrorKind, ParseError};

const I16_NULL_BYTES: [u8; 2] = [0x80, 0x04];
const I32_NULL_BYTES: [u8; 4] = [0xff, 0xff, 0x80, 0x04];
const F64_NULL_BYTES: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];

/// A non-owning reader that reports absolute offsets for every failure.
#[derive(Debug, Clone)]
pub struct BinaryReader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> BinaryReader<'a> {
    /// Create a reader positioned at the start of `data`.
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    /// Create a reader positioned at an absolute byte offset.
    ///
    /// # Errors
    ///
    /// Returns an offset-bearing truncation error if `offset` is beyond `data`.
    pub fn with_position(data: &'a [u8], offset: usize) -> Result<Self, ParseError> {
        if offset > data.len() {
            return Err(ParseError::unexpected_eof(data.len(), offset - data.len(), 0));
        }
        Ok(Self { data, offset })
    }

    /// Return the absolute cursor position.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.offset
    }

    /// Return the complete source length.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.data.len()
    }

    /// Return whether the source contains no bytes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Return the number of bytes after the cursor.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.data.len() - self.offset
    }

    /// Borrow exactly `count` bytes and advance the cursor.
    ///
    /// # Errors
    ///
    /// Returns `binary.truncated_field` at the current cursor when insufficient
    /// bytes remain.
    pub fn bytes(&mut self, count: usize) -> Result<&'a [u8], ParseError> {
        let remaining = self.remaining();
        if count > remaining {
            return Err(ParseError::unexpected_eof(self.offset, count, remaining));
        }
        let start = self.offset;
        self.offset += count;
        Ok(&self.data[start..self.offset])
    }

    /// Read one unsigned byte.
    ///
    /// # Errors
    ///
    /// Returns a truncation error when no byte remains.
    pub fn u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.bytes(1)?[0])
    }

    /// Read a logical byte encoded as exactly zero or one.
    ///
    /// # Errors
    ///
    /// Returns a truncation error or `binary.invalid_logical` at the logical byte.
    pub fn bool8(&mut self) -> Result<bool, ParseError> {
        let offset = self.position();
        let value = self.u8()?;
        match value {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ParseError::invalid_byte(ErrorKind::InvalidLogical, offset, "logical", value)),
        }
    }

    /// Read one big-endian signed 16-bit integer.
    ///
    /// # Errors
    ///
    /// Returns a truncation error when fewer than two bytes remain.
    pub fn i16(&mut self) -> Result<i16, ParseError> {
        let bytes = self.bytes(2)?;
        Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
    }

    /// Read one big-endian unsigned 16-bit integer.
    ///
    /// # Errors
    ///
    /// Returns a truncation error when fewer than two bytes remain.
    pub fn u16(&mut self) -> Result<u16, ParseError> {
        let bytes = self.bytes(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    /// Read a nullable big-endian signed 16-bit integer.
    ///
    /// The observed `80 04` sentinel maps to `None`; other values remain signed.
    ///
    /// # Errors
    ///
    /// Returns a truncation error when fewer than two bytes remain.
    pub fn nullable_i16(&mut self) -> Result<Option<i16>, ParseError> {
        let bytes = self.bytes(2)?;
        if bytes == I16_NULL_BYTES {
            return Ok(None);
        }
        Ok(Some(i16::from_be_bytes([bytes[0], bytes[1]])))
    }

    /// Read one big-endian signed 32-bit integer.
    ///
    /// # Errors
    ///
    /// Returns a truncation error when fewer than four bytes remain.
    pub fn i32(&mut self) -> Result<i32, ParseError> {
        let bytes = self.bytes(4)?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read a nullable big-endian signed 32-bit integer.
    ///
    /// The documented `-32764` sentinel maps to `None`.
    ///
    /// # Errors
    ///
    /// Returns a truncation error when fewer than four bytes remain.
    pub fn nullable_i32(&mut self) -> Result<Option<i32>, ParseError> {
        let bytes = self.bytes(4)?;
        if bytes == I32_NULL_BYTES {
            return Ok(None);
        }
        Ok(Some(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])))
    }

    /// Read a non-negative integer encoded like a compact pointer index.
    ///
    /// Values through 32766 occupy one positive short as `value + 1`. Larger
    /// values use a negative remainder short followed by a positive quotient.
    ///
    /// # Errors
    ///
    /// Returns a truncation error or `binary.invalid_positive_integer` for zero,
    /// `i16::MIN`, or a non-positive quotient.
    pub fn positive_integer(&mut self) -> Result<u32, ParseError> {
        let remainder_offset = self.position();
        let remainder = self.i16()?;
        if remainder > 0 {
            return Ok(u32::from(remainder.unsigned_abs()) - 1);
        }
        if remainder == 0 || remainder == i16::MIN {
            return Err(ParseError::new(
                ErrorKind::InvalidPositiveInteger,
                remainder_offset,
                "positive integer has an invalid encoded remainder",
                crate::ErrorDetails::InvalidLength { field: "positive_integer_remainder", value: i64::from(remainder) },
            ));
        }

        let quotient_offset = self.position();
        let quotient = self.i16()?;
        if quotient <= 0 {
            return Err(ParseError::new(
                ErrorKind::InvalidPositiveInteger,
                quotient_offset,
                "positive integer has a non-positive quotient",
                crate::ErrorDetails::InvalidLength { field: "positive_integer_quotient", value: i64::from(quotient) },
            ));
        }
        let magnitude = u32::from(remainder.unsigned_abs());
        Ok(u32::from(quotient.unsigned_abs()) * 32_767 + magnitude - 1)
    }

    /// Read one big-endian IEEE-754 binary64 value without semantic validation.
    ///
    /// # Errors
    ///
    /// Returns a truncation error when fewer than eight bytes remain.
    pub fn f64(&mut self) -> Result<f64, ParseError> {
        let bytes = self.bytes(8)?;
        Ok(f64::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]]))
    }

    /// Read a binary64 value while preserving the observed null sentinel.
    ///
    /// # Errors
    ///
    /// Returns a truncation error when fewer than eight bytes remain.
    pub fn nullable_f64(&mut self) -> Result<Option<f64>, ParseError> {
        let bytes = self.bytes(8)?;
        if bytes == F64_NULL_BYTES {
            return Ok(None);
        }
        Ok(Some(f64::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]])))
    }

    /// Read a fixed-width ASCII field.
    ///
    /// # Errors
    ///
    /// Returns a truncation error or `binary.invalid_ascii` at the first invalid byte.
    pub fn ascii(&mut self, count: usize) -> Result<String, ParseError> {
        let start = self.position();
        let bytes = self.bytes(count)?;
        if let Some((index, value)) = bytes.iter().copied().enumerate().find(|(_, value)| !value.is_ascii()) {
            return Err(ParseError::invalid_byte(ErrorKind::InvalidAscii, start + index, "ASCII text", value));
        }
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    /// Read ASCII text with an unsigned-byte length prefix.
    ///
    /// # Errors
    ///
    /// Returns a truncation or invalid-ASCII error.
    pub fn ascii_u8_length(&mut self) -> Result<String, ParseError> {
        let count = usize::from(self.u8()?);
        self.ascii(count)
    }

    /// Read ASCII text with an unsigned 16-bit length prefix and size bound.
    ///
    /// # Errors
    ///
    /// Returns an offset-bearing limit, truncation, or ASCII error.
    pub fn ascii_u16_length(&mut self, max_length: usize) -> Result<String, ParseError> {
        let length_offset = self.position();
        let count = usize::from(self.u16()?);
        if count > max_length {
            return Err(ParseError::limit(length_offset, "string_bytes", count, max_length));
        }
        self.ascii(count)
    }

    /// Read ASCII text with a signed 32-bit length prefix and explicit size bound.
    ///
    /// # Errors
    ///
    /// Returns an offset-bearing invalid-length, limit, truncation, or ASCII error.
    pub fn ascii_i32_length(&mut self, max_length: usize, field: &'static str) -> Result<String, ParseError> {
        let length_offset = self.position();
        let serialized = self.i32()?;
        let count = usize::try_from(serialized).map_err(|_| ParseError::invalid_length(length_offset, field, i64::from(serialized)))?;
        if count > max_length {
            return Err(ParseError::limit(length_offset, "string_bytes", count, max_length));
        }
        self.ascii(count)
    }

    /// Read a fixed number of UTF-16BE code units.
    ///
    /// # Errors
    ///
    /// Returns a limit, truncation, or `binary.invalid_utf16` error.
    pub fn utf16_be(&mut self, code_units: usize) -> Result<String, ParseError> {
        let byte_count =
            code_units.checked_mul(2).ok_or_else(|| ParseError::limit(self.position(), "utf16_code_units", code_units, usize::MAX / 2))?;
        let start = self.position();
        let bytes = self.bytes(byte_count)?;
        let units = bytes.chunks_exact(2).map(|pair| u16::from_be_bytes([pair[0], pair[1]]));
        let mut text = String::new();
        for (index, decoded) in char::decode_utf16(units).enumerate() {
            match decoded {
                Ok(value) => text.push(value),
                Err(_) => {
                    return Err(ParseError::new(
                        ErrorKind::InvalidUtf16,
                        start + index * 2,
                        "UTF-16BE text contains an unpaired surrogate",
                        crate::ErrorDetails::None,
                    ));
                }
            }
        }
        Ok(text)
    }

    /// Read two consecutive binary64 values.
    ///
    /// # Errors
    ///
    /// Returns a truncation error at the first incomplete scalar.
    pub fn interval(&mut self) -> Result<[f64; 2], ParseError> {
        Ok([self.f64()?, self.f64()?])
    }

    /// Read three consecutive binary64 values.
    ///
    /// # Errors
    ///
    /// Returns a truncation error at the first incomplete scalar.
    pub fn vector(&mut self) -> Result<[f64; 3], ParseError> {
        Ok([self.f64()?, self.f64()?, self.f64()?])
    }

    /// Read three consecutive intervals.
    ///
    /// # Errors
    ///
    /// Returns a truncation error at the first incomplete scalar.
    pub fn box3(&mut self) -> Result<[[f64; 2]; 3], ParseError> {
        Ok([self.interval()?, self.interval()?, self.interval()?])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorDetails, ErrorKind};

    #[test]
    fn reads_big_endian_primitives_and_composites() {
        let mut data = Vec::new();
        data.extend_from_slice(&[7, 1]);
        data.extend_from_slice(&(-123_i16).to_be_bytes());
        data.extend_from_slice(&65_000_u16.to_be_bytes());
        data.extend_from_slice(&123_456_i32.to_be_bytes());
        for value in [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0] {
            data.extend_from_slice(&value.to_be_bytes());
        }
        let mut reader = BinaryReader::new(&data);

        assert_eq!(reader.u8(), Ok(7));
        assert_eq!(reader.bool8(), Ok(true));
        assert_eq!(reader.i16(), Ok(-123));
        assert_eq!(reader.u16(), Ok(65_000));
        assert_eq!(reader.i32(), Ok(123_456));
        assert_eq!(reader.box3(), Ok([[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]));
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn preserves_observed_null_sentinels() {
        let mut data = Vec::from(I16_NULL_BYTES);
        data.extend_from_slice(&I32_NULL_BYTES);
        data.extend_from_slice(&F64_NULL_BYTES);
        let mut reader = BinaryReader::new(&data);

        assert_eq!(reader.nullable_i16(), Ok(None));
        assert_eq!(reader.nullable_i32(), Ok(None));
        assert_eq!(reader.nullable_f64(), Ok(None));
    }

    #[test]
    fn reports_exact_truncation_offset_and_counts() {
        let mut reader = BinaryReader::new(&[0x01, 0x02, 0x03]);
        assert_eq!(reader.u8(), Ok(1));
        let error = reader.i32().err();

        assert!(error.is_some());
        let error = error.as_ref();
        assert_eq!(error.map(ParseError::kind), Some(ErrorKind::UnexpectedEof));
        assert_eq!(error.map(ParseError::offset), Some(1));
        assert_eq!(error.map(ParseError::details), Some(&ErrorDetails::UnexpectedEof { needed: 4, remaining: 2 }));
    }

    #[test]
    fn rejects_invalid_logical_at_its_byte() {
        let mut reader = BinaryReader::new(&[0, 2]);
        assert_eq!(reader.bool8(), Ok(false));

        let error = reader.bool8().err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidLogical));
        assert_eq!(error.as_ref().map(ParseError::offset), Some(1));
    }

    #[test]
    fn rejects_non_ascii_at_its_byte() {
        let mut reader = BinaryReader::new(b"ok\xff");
        let error = reader.ascii(3).err();

        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidAscii));
        assert_eq!(error.as_ref().map(ParseError::offset), Some(2));
    }

    #[test]
    fn reads_utf16be_and_locates_unpaired_surrogate() {
        let mut valid = BinaryReader::new(&[0x00, 0x41, 0xd8, 0x3d, 0xde, 0x00]);
        assert_eq!(valid.utf16_be(3), Ok("A😀".to_owned()));

        let mut invalid = BinaryReader::new(&[0x00, 0x41, 0xd8, 0x00]);
        let error = invalid.utf16_be(2).err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidUtf16));
        assert_eq!(error.as_ref().map(ParseError::offset), Some(2));
    }

    #[test]
    fn bounds_i32_prefixed_ascii_before_allocation() {
        let mut data = Vec::from(5_i32.to_be_bytes());
        data.extend_from_slice(b"hello");
        let mut reader = BinaryReader::new(&data);

        let error = reader.ascii_i32_length(4, "test").err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::LimitExceeded));
        assert_eq!(error.as_ref().map(ParseError::offset), Some(0));
    }

    #[test]
    fn reads_and_bounds_u16_prefixed_ascii() {
        let mut data = Vec::from(5_u16.to_be_bytes());
        data.extend_from_slice(b"hello");

        let mut valid = BinaryReader::new(&data);
        assert_eq!(valid.ascii_u16_length(5), Ok("hello".to_owned()));

        let mut limited = BinaryReader::new(&data);
        let error = limited.ascii_u16_length(4).err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::LimitExceeded));
        assert_eq!(error.as_ref().map(ParseError::offset), Some(0));
    }

    #[test]
    fn reads_small_and_large_positive_integers() {
        let mut data = Vec::new();
        data.extend_from_slice(&1_i16.to_be_bytes());
        data.extend_from_slice(&32_767_i16.to_be_bytes());
        data.extend_from_slice(&(-2_i16).to_be_bytes());
        data.extend_from_slice(&3_i16.to_be_bytes());
        let mut reader = BinaryReader::new(&data);

        assert_eq!(reader.positive_integer(), Ok(0));
        assert_eq!(reader.positive_integer(), Ok(32_766));
        assert_eq!(reader.positive_integer(), Ok(3 * 32_767 + 1));
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn rejects_invalid_positive_integer_sequences_at_exact_short() {
        for (data, expected_offset) in [([0_u8, 0, 0, 0], 0), ([0x80, 0, 0, 1], 0), ([0xff, 0xff, 0, 0], 2)] {
            let error = BinaryReader::new(&data).positive_integer().err();
            assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidPositiveInteger));
            assert_eq!(error.as_ref().map(ParseError::offset), Some(expected_offset));
        }
    }
}
