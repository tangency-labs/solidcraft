// SPDX-License-Identifier: Apache-2.0
//! Text reads shared by Design record decoders.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

/// Validate a borrowed three-digit class tag before making its fixed-size copy.
pub(in crate::design::decode) fn class_tag_from_view(
    value: &str,
) -> Result<crate::records::references::DesignClassTag, String> {
    if value.len() != 3 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("class_tag must contain three ASCII digits".into());
    }
    crate::records::references::DesignClassTag::try_from(value.to_owned())
}

/// Compose a native scope and record suffix.
pub(in crate::design::decode) fn design_record_id(
    stream: &str,
    suffix: &'static str,
    offset: u64,
) -> String {
    format!("{}{suffix}{offset}", crate::ids::native_scope(stream))
}

/// Validate an exact 36-code-unit relaxed GUID into a fixed ASCII array.
pub(in crate::design::decode) fn fixed_guid_ascii(
    bytes: &[u8],
    count_at: usize,
) -> Option<([u8; 36], usize)> {
    (View::u32_le_at(bytes, count_at)? == 36).then_some(())?;
    let start = count_at.checked_add(4)?;
    let end = start.checked_add(72)?;
    let mut guid = [0; 36];
    for (slot, unit) in guid.iter_mut().zip(bytes.get(start..end)?.chunks_exact(2)) {
        if unit[1] != 0 || !(unit[0].is_ascii_alphanumeric() || matches!(unit[0], b'-' | b'_')) {
            return None;
        }
        *slot = unit[0];
    }
    Some((guid, end))
}

/// Read a fixed-width relaxed GUID into its native value after code-unit validation.
pub(in crate::design::decode) fn fixed_relaxed_guid_text(
    bytes: &[u8],
    count_at: usize,
) -> Result<Option<(crate::records::mesh::DesignRelaxedGuidText, usize)>, CodecError> {
    let Some((guid, end)) = fixed_guid_ascii(bytes, count_at) else {
        return Ok(None);
    };
    let guid = std::str::from_utf8(&guid)
        .map_err(|_| CodecError::malformed("validated F3D relaxed GUID is not ASCII"))?;
    let text = guid.to_owned();
    let value = crate::records::mesh::DesignRelaxedGuidText::try_from(text)
        .map_err(CodecError::malformed)?;
    Ok(Some((value, end)))
}

/// Read an LP-UTF16 occurrence key: a positive decimal key or an exact
/// 36-code-unit relaxed GUID.
pub(in crate::design::decode) fn occurrence_key_text(
    bytes: &[u8],
    count_at: usize,
) -> Result<Option<(crate::records::mesh::DesignOccurrenceKey, usize)>, CodecError> {
    if let Some((key, end)) = crate::bytes::lp_utf16_bounded(bytes, count_at, 1..=9) {
        if crate::records::mesh::DesignOccurrenceKey::is_decimal_key(&key) {
            let key = crate::records::mesh::DesignOccurrenceKey::try_from(key)
                .map_err(CodecError::malformed)?;
            return Ok(Some((key, end)));
        }
    }
    Ok(fixed_relaxed_guid_text(bytes, count_at)?.map(|(guid, end)| (guid.into(), end)))
}

/// Validate an exact 36-code-unit relaxed GUID in UTF-16LE without copying it.
pub(in crate::design::decode) fn fixed_guid_end(bytes: &[u8], count_at: usize) -> Option<usize> {
    fixed_guid_ascii(bytes, count_at).map(|(_, end)| end)
}

/// Validate a counted relaxed GUID without allocating its text.
pub(in crate::design::decode) fn relaxed_guid_end(bytes: &[u8], count_at: usize) -> Option<usize> {
    let count = usize::try_from(View::u32_le_at(bytes, count_at)?).ok()?;
    if !(36..=38).contains(&count) {
        return None;
    }
    let start = count_at.checked_add(4)?;
    let end = start.checked_add(count.checked_mul(2)?)?;
    bytes
        .get(start..end)?
        .chunks_exact(2)
        .all(|unit| {
            unit[1] == 0 && (unit[0].is_ascii_alphanumeric() || matches!(unit[0], b'-' | b'_'))
        })
        .then_some(end)
}

/// Match an ASCII literal encoded as a counted UTF-16LE field without copying it.
pub(in crate::design::decode) fn fixed_utf16_ascii_eq(
    bytes: &[u8],
    count_at: usize,
    expected: &str,
) -> Option<usize> {
    if !expected.is_ascii()
        || usize::try_from(View::u32_le_at(bytes, count_at)?).ok()? != expected.len()
    {
        return None;
    }
    let start = count_at.checked_add(4)?;
    let end = start.checked_add(expected.len().checked_mul(2)?)?;
    bytes
        .get(start..end)?
        .chunks_exact(2)
        .zip(expected.bytes())
        .all(|(unit, expected_byte)| unit == [expected_byte, 0])
        .then_some(end)
}

#[cfg(test)]
mod tests {
    use super::{class_tag_from_view, fixed_guid_ascii, fixed_utf16_ascii_eq, relaxed_guid_end};
    use crate::bytes::lp_ascii_filtered_view;

    #[test]
    fn fixed_relaxed_guid_text_preserves_bytes() {
        let value = "ABCDEF12-3456-7890-ABCD-EF1234567890";
        let bytes = crate::bytes::lp_utf16_bytes(value).unwrap();
        let (guid, end) = super::fixed_relaxed_guid_text(&bytes, 0).unwrap().unwrap();
        assert_eq!(guid.as_str(), value);
        assert_eq!(end, bytes.len());
    }

    #[test]
    fn relaxed_guid_scan_matches_owned_validation_at_each_admitted_length() {
        for value in [
            "00000000-0000-0000-0000-000000000000",
            "00000000-0000-0000-0000-000000000000A",
            "00000000-0000-0000-0000-000000000000AB",
            "00000000-0000-0000-0000-000000000000ABC",
            "00000000-0000-0000-0000-00000000000!",
        ] {
            let mut bytes = u32::try_from(value.encode_utf16().count())
                .unwrap()
                .to_le_bytes()
                .to_vec();
            for unit in value.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            let owned = crate::bytes::lp_utf16_bounded(&bytes, 0, 1..=256)
                .and_then(|(value, end)| crate::bytes::is_guid_relaxed(&value).then_some(end));
            assert_eq!(relaxed_guid_end(&bytes, 0), owned);
        }
    }

    #[test]
    fn fixed_guid_ascii_preserves_decoded_text() {
        for value in [
            "ABCDEF12-3456-7890-ABCD-EF1234567890",
            "00000000-0000-0000-0000-000000000000",
            "00000000-0000-0000-0000-00000000000!",
            "é0000000-0000-0000-0000-000000000000",
        ] {
            let mut bytes = u32::try_from(value.encode_utf16().count())
                .unwrap()
                .to_le_bytes()
                .to_vec();
            for unit in value.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            let prior = crate::bytes::lp_utf16_bounded(&bytes, 0, 36..=36)
                .filter(|(text, _)| crate::bytes::is_guid_relaxed(text));
            let current = fixed_guid_ascii(&bytes, 0)
                .map(|(guid, end)| (String::from_utf8(guid.to_vec()).unwrap(), end));
            assert_eq!(current, prior);
        }
    }

    #[test]
    fn fixed_utf16_ascii_match_agrees_with_decoded_text() {
        for (value, expected) in [
            ("Thicken", "Thicken"),
            ("Thicken", "Shell"),
            ("Shell", "Shell"),
            ("Thickén", "Thicken"),
        ] {
            let mut bytes = u32::try_from(value.encode_utf16().count())
                .unwrap()
                .to_le_bytes()
                .to_vec();
            for unit in value.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            let decoded =
                { crate::bytes::lp_utf16_bounded(&bytes, 0, expected.len()..=expected.len()) }
                    .and_then(|(text, end)| (text == expected).then_some(end));
            assert_eq!(fixed_utf16_ascii_eq(&bytes, 0, expected), decoded);
            bytes.pop();
            assert_eq!(fixed_utf16_ascii_eq(&bytes, 0, expected), None);
        }
    }

    #[test]
    fn borrowed_ascii_reader_matches_owned_reader() {
        let fields: [&[u8]; 6] = [b"", b"123", b"EntityGenesis", b"a-b_", b"\0", b"\x80"];
        for field in fields {
            let mut bytes = u32::try_from(field.len()).unwrap().to_le_bytes().to_vec();
            bytes.extend_from_slice(field);
            for bounds in [0..=2000, 3..=3] {
                let predicates: [fn(&u8) -> bool; 2] = [u8::is_ascii_graphic, u8::is_ascii_digit];
                for allowed in predicates {
                    let original = crate::bytes::lp_ascii_strict(&bytes, 0, bounds.clone())
                        .filter(|(text, _)| text.as_bytes().iter().all(allowed));
                    let borrowed = lp_ascii_filtered_view(&bytes, 0, bounds.clone(), allowed);
                    assert_eq!(borrowed, original);
                    bytes.pop();
                    assert_eq!(
                        lp_ascii_filtered_view(&bytes, 0, bounds.clone(), allowed),
                        None
                    );
                    bytes.push(*field.last().unwrap_or(&0));
                }
            }
        }
    }

    #[test]
    fn borrowed_class_tag_conversion_matches_owned_conversion() {
        for value in ["123", "000", "12", "1234", "12a", "éé"] {
            assert_eq!(
                class_tag_from_view(value),
                crate::records::references::DesignClassTag::try_from(value.to_owned())
            );
        }
    }
}
