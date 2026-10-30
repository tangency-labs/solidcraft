//! Nine-alignment TEXT adapter over the real parsed Ent. No parser or glyph layout.
//! Registration and the production call remain with the parent dxf module owner.

use solidcraft_geom::Vec2;

use super::text_value::decode_text_value;
use super::{Ent, MAX_WORK, layered_array_budget, layered_number, layered_required, layered_text};
use crate::dxf_out::{DxfTextHorizontalAlignment, DxfTextVerticalAlignment};
use crate::{DxfShape, IoError, MAX_DRAWING_BYTES, Result};

const MAX_TEXT_ENCODED_BYTES: usize = 250 * 7;
const MAX_TEXT_DECODED_BYTES: usize = 250 * 3;

fn invalid(reason: &str) -> IoError {
    IoError::Invalid(format!("layered DXF TEXT: {reason}"))
}

fn number(entity: &Ent, code: i32) -> Result<Option<f64>> {
    let value = layered_number(entity, code)?;
    if value.is_some_and(|value| !value.is_finite()) {
        return Err(invalid("nonfinite numeric field"));
    }
    Ok(value)
}

fn required(entity: &Ent, code: i32) -> Result<f64> {
    let value = layered_required(entity, code)?;
    if !value.is_finite() {
        return Err(invalid("nonfinite required field"));
    }
    Ok(value)
}

/// Read nine-alignment model-space TEXT; return an error for MTEXT and any
/// unrepresented alignment/style/OCS. Input is raw Ent.s, decoded exactly once.
/// Both counters belong to the whole file; callers must not reset them per TEXT.
/// Decoded text is restricted to the current writer's faithful plain BMP subset.
pub(super) fn read_text_entity(
    entity: &Ent,
    k: f64,
    max_encoded_bytes: usize,
    max_decoded_bytes: usize,
    bytes: &mut usize,
    work: &mut usize,
) -> Result<DxfShape> {
    if entity.kind != "TEXT" {
        return Err(invalid("not supported yet: MTEXT attachment/wrapping or a non-TEXT entity"));
    }
    if !k.is_finite() || k <= 0.0 {
        return Err(invalid("unit scale must be finite and positive"));
    }
    if max_encoded_bytes > MAX_DRAWING_BYTES || max_decoded_bytes > MAX_DRAWING_BYTES {
        return Err(invalid("decoder byte caps exceed the file ceiling"));
    }
    let remaining_bytes = MAX_DRAWING_BYTES.checked_sub(*bytes).ok_or_else(|| invalid("invalid shared byte counter"))?;
    let remaining_work = MAX_WORK.checked_sub(*work).ok_or_else(|| invalid("invalid shared work counter"))?;

    // Known envelope fields retain the parent's layer/weight/space semantics.
    // Unknown geometry, explicit style7, MTEXT chunks3,
    // entity color/visibility/linetype and extension data cannot silently vanish.
    for code in entity.f.iter().map(|(code, _)| *code).chain(entity.s.iter().map(|(code, _)| *code)) {
        if !matches!(code, 1 | 5 | 8 | 10 | 11 | 20 | 21 | 30 | 31 | 39 | 40 | 41 | 50 | 51 | 67 | 71 | 72 | 73 | 100 | 210 | 220 | 230 | 330 | 370) {
            return Err(invalid("not supported yet: unrepresented field or style"));
        }
    }
    for code in [5, 8, 330] {
        let _ = layered_text(entity, code)?;
    }
    // Repeated subclass markers100 are ordinary TEXT framing, not scalar fields.
    let _ = super::layered_entity_weight(entity)?;
    if number(entity, 67)?.unwrap_or(0.0) != 0.0 {
        return Err(invalid("expected model-space TEXT after caller space filtering"));
    }
    for (code, neutral) in [(30, 0.0), (31, 0.0), (39, 0.0), (41, 1.0), (51, 0.0), (71, 0.0), (210, 0.0), (220, 0.0), (230, 1.0)] {
        if number(entity, code)?.is_some_and(|value| value != neutral) {
            return Err(invalid("not supported yet: nondefault alignment, width, oblique angle, generation flags or OCS"));
        }
    }
    // Parent lexical preflight must validate72/73 before section converts them
    // through f64. These finite local enum checks also protect direct Ent callers.
    let horizontal = match number(entity, 72)?.unwrap_or(0.0) {
        0.0 => DxfTextHorizontalAlignment::Left,
        1.0 => DxfTextHorizontalAlignment::Centre,
        2.0 => DxfTextHorizontalAlignment::Right,
        _ => return Err(invalid("not supported yet: horizontal TEXT alignment")),
    };
    let vertical = match number(entity, 73)?.unwrap_or(0.0) {
        0.0 => DxfTextVerticalAlignment::Baseline,
        2.0 => DxfTextVerticalAlignment::Middle,
        3.0 => DxfTextVerticalAlignment::Top,
        _ => return Err(invalid("not supported yet: vertical TEXT alignment")),
    };
    let insertion = Vec2::new(required(entity, 10)?, required(entity, 20)?);
    let source = if horizontal == DxfTextHorizontalAlignment::Left && vertical == DxfTextVerticalAlignment::Baseline {
        // Optional second point is irrelevant for left/baseline, but malformed
        // or duplicate fields still refuse. No silent fallback for other modes.
        let _ = number(entity, 11)?;
        let _ = number(entity, 21)?;
        insertion
    } else {
        Vec2::new(required(entity, 11)?, required(entity, 21)?)
    };
    let at = source * k;
    if !at.is_finite() || (source.x != 0.0 && at.x == 0.0) || (source.y != 0.0 && at.y == 0.0) {
        return Err(invalid("nonfinite or collapsed scaled insertion point"));
    }
    let raw_height = required(entity, 40)?;
    let height = raw_height * k;
    if raw_height <= 0.0 || !height.is_finite() || height <= 0.0 {
        return Err(invalid("height must remain finite and positive after scaling"));
    }
    let angle_deg = number(entity, 50)?.unwrap_or(0.0);
    if !angle_deg.to_radians().is_finite() {
        return Err(invalid("invalid rotation angle"));
    }
    let value = layered_text(entity, 1)?.ok_or_else(|| invalid("missing raw group1 value"))?;
    // Check input before allocation/scanning. Decoded output cannot allocate past
    // remaining shared payload/work even when its UTF8 representation expands.
    let encoded_limit = max_encoded_bytes.min(MAX_TEXT_ENCODED_BYTES);
    if value.len() > encoded_limit || value.len() > remaining_work {
        return Err(invalid("encoded byte or shared work budget exceeded"));
    }
    *work = work.checked_add(value.len()).ok_or_else(|| invalid("work counter overflow"))?;
    // Current owned writer accepts at most250 BMP scalars: at most750 UTF8 bytes.
    let decoded_limit = max_decoded_bytes.min(remaining_bytes).min(remaining_work - value.len()).min(MAX_TEXT_DECODED_BYTES);
    let text = decode_text_value(value, encoded_limit, decoded_limit)?;
    if text.chars().count() > 250 || text.chars().any(|ch| u32::from(ch) > 0xffff || ch.is_control() || matches!(ch, '%' | '\\')) {
        return Err(invalid("not supported yet: decoded text exceeds the current writer's plain BMP subset"));
    }
    layered_array_budget::<u8>(text.len(), bytes, work)?;
    if horizontal == DxfTextHorizontalAlignment::Centre && vertical == DxfTextVerticalAlignment::Baseline {
        Ok(DxfShape::Text { at, height, angle_deg, text })
    } else {
        Ok(DxfShape::AlignedText { at, height, angle_deg, text, horizontal, vertical })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIELDS: &str = "10\n99\n20\n88\n11\n1\n21\n2\n40\n3\n72\n1\n1\nA\n";

    fn parsed(fields: &str) -> Ent {
        let input = format!("0\nSECTION\n2\nENTITIES\n0\nTEXT\n{fields}0\nENDSEC\n0\nEOF\n");
        super::super::section(&super::super::pairs(&input), 1).unwrap().remove(0)
    }

    fn basic(value: &str) -> Ent {
        parsed(&FIELDS.replacen("1\nA\n", &format!("1\n{value}\n"), 1))
    }

    fn read(entity: &Ent) -> Result<DxfShape> {
        read_text_entity(entity, 1.0, 4096, 4096, &mut 0, &mut 0)
    }

    #[test]
    fn raw_text_uses_real_decoder_and_second_alignment_point_without_trimming() {
        assert_eq!(
            read(&basic("  %%d %%p %%c \\U+4E2D  ")).unwrap(),
            DxfShape::Text { at: Vec2::new(1.0, 2.0), height: 3.0, angle_deg: 0.0, text: "  ° ± \u{2205} 中  ".into() }
        );
        for value in ["", "   ", "ASCII braces {}"] {
            assert!(matches!(read(&basic(value)).unwrap(), DxfShape::Text { text, .. } if text == value));
        }
    }

    #[test]
    fn scale_alignment_point_and_height_once_keep_rotation_in_degrees() {
        let fields = FIELDS.replacen("40\n3\n", "40\n0.5\n50\n-90\n", 1);
        assert_eq!(
            read_text_entity(&parsed(&fields), 25.4, 1, 1, &mut 0, &mut 0).unwrap(),
            DxfShape::Text { at: Vec2::new(25.4, 50.8), height: 12.7, angle_deg: -90.0, text: "A".into() }
        );
    }

    #[test]
    fn neutral_fields_and_repeated_real_subclasses_are_admitted() {
        let entity = parsed(&format!(
            "{FIELDS}100\nAcDbEntity\n100\nAcDbText\n100\nAcDbText\n30\n0\n31\n0\n39\n0\n41\n1\n50\n0\n51\n0\n71\n0\n73\n0\n210\n0\n220\n0\n230\n1\n"
        ));
        assert!(read(&entity).is_ok());
    }

    #[test]
    fn missing_malformed_and_duplicate_fields_refuse() {
        for removed in ["10\n99\n", "20\n88\n", "11\n1\n", "21\n2\n", "40\n3\n", "1\nA\n"] {
            assert!(read(&parsed(&FIELDS.replacen(removed, "", 1))).is_err(), "{removed}");
        }
        for extra in ["10\n99\n", "11\n1\n", "21\n2\n", "40\n3\n", "72\n1\n", "1\nA\n", "50\n0\n50\n0\n", "73\n0\n73\n0\n", "230\n1\n230\n1\n"] {
            assert!(read(&parsed(&format!("{FIELDS}{extra}"))).is_err(), "{extra}");
        }
        for (from, to) in [("11\n1", "11\nNaN"), ("21\n2", "21\nbad"), ("40\n3", "40\ninf"), ("72\n1", "72\nwrong")] {
            assert!(read(&parsed(&FIELDS.replacen(from, to, 1))).is_err());
        }
        let mut entity = basic("A");
        entity.f.push((1, 1.0));
        assert!(read(&entity).is_err());
    }

    #[test]
    fn unsupported_alignment_style_ocs_and_mtext_remain_explicit_errors() {
        for mode in ["-1", "3", "4", "5", "0.5"] {
            assert!(read(&parsed(&FIELDS.replacen("72\n1\n", &format!("72\n{mode}\n"), 1))).is_err());
        }
        for extra in [
            "7\nSTANDARD\n",
            "7\nCustom\n",
            "41\n2\n",
            "51\n15\n",
            "71\n2\n",
            "73\n1\n",
            "30\n1\n",
            "31\n1\n",
            "39\n1\n",
            "210\n1\n",
            "220\n1\n",
            "230\n-1\n",
            "3\nextra\n",
            "6\nDASHED\n",
            "62\n1\n",
            "60\n1\n",
            "67\n1\n",
            "999\ncomment\n",
        ] {
            assert!(read(&parsed(&format!("{FIELDS}{extra}"))).is_err(), "{extra}");
        }
        let mut entity = basic("A");
        entity.kind = "MTEXT".into();
        assert!(read(&entity).unwrap_err().to_string().contains("MTEXT attachment/wrapping"));
    }

    #[test]
    fn nonrepresentable_literals_are_refused_after_exactly_one_decode() {
        for value in ["%", "%%%", "\\U+0025\\U+0025d", "\\U+005CU+0041", "\\P", "a\tb", "中𠀋", "\\U+D800", "\\U+0000", "%%q"] {
            assert!(read(&basic(value)).is_err(), "{value}");
        }
        assert!(read(&basic(&"中".repeat(250))).is_ok());
        assert!(read(&basic(&"A".repeat(251))).is_err());
        assert!(read(&basic(&"中".repeat(251))).is_err());
    }

    #[test]
    fn invalid_geometry_scale_and_underflow_refuse() {
        for k in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MAX] {
            assert!(read_text_entity(&basic("A"), k, 1, 1, &mut 0, &mut 0).is_err());
        }
        for (code, value) in [(10, f64::NAN), (11, f64::INFINITY), (21, f64::NAN), (40, 0.0), (40, -1.0), (40, f64::NAN), (50, f64::NAN)] {
            let mut entity = basic("A");
            entity.f.retain(|(field, _)| *field != code);
            entity.f.push((code, value));
            assert!(read(&entity).is_err());
        }
        for fields in [FIELDS.replacen("11\n1\n", "11\n1e-300\n", 1), FIELDS.replacen("40\n3\n", "40\n1e-300\n", 1)] {
            assert!(read_text_entity(&parsed(&fields), 1e-100, 1, 1, &mut 0, &mut 0).is_err());
        }
    }

    #[test]
    fn inclusive_decoder_caps_measure_utf8_bytes() {
        let entity = basic("\\U+FFFF");
        assert!(read_text_entity(&entity, 1.0, 7, 3, &mut 0, &mut 0).is_ok());
        assert!(read_text_entity(&entity, 1.0, 6, 3, &mut 0, &mut 0).is_err());
        assert!(read_text_entity(&entity, 1.0, 7, 2, &mut 0, &mut 0).is_err());
        assert!(read_text_entity(&basic("中"), 1.0, 2, 3, &mut 0, &mut 0).is_err());
        assert!(read_text_entity(&basic(""), 1.0, 0, 0, &mut 0, &mut 0).is_ok());
        assert!(read_text_entity(&basic("A"), 1.0, MAX_DRAWING_BYTES + 1, 1, &mut 0, &mut 0).is_err());
        assert!(read_text_entity(&basic("A"), 1.0, 1, MAX_DRAWING_BYTES + 1, &mut 0, &mut 0).is_err());
    }

    #[test]
    fn shared_counters_remain_cumulative_and_limit_before_decode_allocation() {
        let entity = basic("A");
        let mut bytes = MAX_DRAWING_BYTES - 2;
        let mut work = MAX_WORK - 4;
        assert!(read_text_entity(&entity, 1.0, 1, 1, &mut bytes, &mut work).is_ok());
        assert!(read_text_entity(&entity, 1.0, 1, 1, &mut bytes, &mut work).is_ok());
        assert_eq!((bytes, work), (MAX_DRAWING_BYTES, MAX_WORK));
        assert!(read_text_entity(&entity, 1.0, 1, 1, &mut bytes, &mut work).is_err());
        let mut full_bytes = MAX_DRAWING_BYTES;
        assert!(read_text_entity(&entity, 1.0, 1, 1, &mut full_bytes, &mut 0).is_err());
        assert!(read_text_entity(&entity, 1.0, 1, 1, &mut 0, &mut (MAX_WORK - 1)).is_err());
        let mut invalid_bytes = usize::MAX;
        assert!(read_text_entity(&entity, 1.0, 1, 1, &mut invalid_bytes, &mut 0).is_err());
        let mut invalid_work = usize::MAX;
        assert!(read_text_entity(&entity, 1.0, 1, 1, &mut 0, &mut invalid_work).is_err());
    }

    #[test]
    fn parent_lexical_preflight_must_precede_float_field_conversion() {
        for extra in ["71\n1e-400\n", "73\n0.0\n"] {
            let input = format!("0\nSECTION\n2\nENTITIES\n0\nTEXT\n{FIELDS}{extra}0\nENDSEC\n0\nEOF\n");
            assert!(super::super::layered_array_integer_fields(&super::super::pairs(&input), 1).is_err());
        }
        let fields = FIELDS.replacen("72\n1\n", "72\n1.0000000000000000001\n", 1);
        let input = format!("0\nSECTION\n2\nENTITIES\n0\nTEXT\n{fields}0\nENDSEC\n0\nEOF\n");
        assert!(super::super::layered_array_integer_fields(&super::super::pairs(&input), 1).is_err());
    }

    #[test]
    fn nine_literal_mode_pairs_select_the_actual_anchor_and_preserve_legacy_text() {
        use DxfTextHorizontalAlignment::{Centre, Left, Right};
        use DxfTextVerticalAlignment::{Baseline, Middle, Top};
        for (h, v, horizontal, vertical) in [
            (0, 0, Left, Baseline),
            (1, 0, Centre, Baseline),
            (2, 0, Right, Baseline),
            (0, 2, Left, Middle),
            (1, 2, Centre, Middle),
            (2, 2, Right, Middle),
            (0, 3, Left, Top),
            (1, 3, Centre, Top),
            (2, 3, Right, Top),
        ] {
            let fields = FIELDS.replacen("72\n1\n", &format!("72\n{h}\n73\n{v}\n50\n-90\n"), 1);
            let shape = read_text_entity(&parsed(&fields), 2.0, 1, 1, &mut 0, &mut 0).unwrap();
            let at = if h == 0 && v == 0 { Vec2::new(198.0, 176.0) } else { Vec2::new(2.0, 4.0) };
            let expected = if h == 1 && v == 0 {
                DxfShape::Text { at, height: 6.0, angle_deg: -90.0, text: "A".into() }
            } else {
                DxfShape::AlignedText { at, height: 6.0, angle_deg: -90.0, text: "A".into(), horizontal, vertical }
            };
            assert_eq!(shape, expected, "72={h},73={v}");
        }
    }

    #[test]
    fn default_left_baseline_uses_first_point_without_requiring_second_point() {
        let fields = FIELDS.replacen("72\n1\n", "", 1).replacen("11\n1\n", "", 1).replacen("21\n2\n", "", 1);
        assert_eq!(
            read(&parsed(&fields)).unwrap(),
            DxfShape::AlignedText {
                at: Vec2::new(99.0, 88.0),
                height: 3.0,
                angle_deg: 0.0,
                text: "A".into(),
                horizontal: DxfTextHorizontalAlignment::Left,
                vertical: DxfTextVerticalAlignment::Baseline,
            }
        );
        // Though irrelevant for placement, a present malformed/duplicate point
        // cannot be used to hide corrupted input in the default mode.
        for extra in ["11\nbad\n", "21\nNaN\n", "11\n1\n11\n1\n"] {
            assert!(read(&parsed(&format!("{fields}{extra}"))).is_err());
        }
    }

    #[test]
    fn every_other_mode_requires_second_anchor_and_never_falls_back_to_first() {
        for (h, v) in [(1, 0), (2, 0), (0, 2), (1, 2), (2, 2), (0, 3), (1, 3), (2, 3)] {
            let fields = FIELDS.replacen("72\n1\n", &format!("72\n{h}\n73\n{v}\n"), 1);
            for removed in ["11\n1\n", "21\n2\n"] {
                assert!(read(&parsed(&fields.replacen(removed, "", 1))).is_err());
            }
        }
        for invalid in ["73\n-1\n", "73\n1\n", "73\n4\n", "73\n0.5\n", "73\n2\n73\n2\n"] {
            assert!(read(&parsed(&format!("{FIELDS}{invalid}"))).is_err());
        }
    }

    #[test]
    fn two_hundred_fifty_cif_bmp_chars_need_1750_encoded_and_750_decoded_bytes() {
        let encoded = "\\U+4E2D".repeat(250);
        let expected = "中".repeat(250);
        assert_eq!(encoded.len(), 1750);
        assert_eq!(expected.len(), 750);
        let entity = basic(&encoded);
        let (mut bytes, mut work) = (0, 0);
        assert_eq!(
            read_text_entity(&entity, 1.0, 1750, 750, &mut bytes, &mut work).unwrap(),
            DxfShape::Text { at: Vec2::new(1.0, 2.0), height: 3.0, angle_deg: 0.0, text: expected }
        );
        assert_eq!((bytes, work), (750, 2500));
        let (mut bytes, mut work) = (0, 0);
        assert!(read_text_entity(&entity, 1.0, 1749, 750, &mut bytes, &mut work).is_err());
        assert_eq!((bytes, work), (0, 0), "encoded admission precedes decoding");
        assert!(read_text_entity(&entity, 1.0, 1750, 749, &mut 0, &mut 0).is_err());
        assert!(read_text_entity(&entity, 1.0, 1750, 750, &mut (MAX_DRAWING_BYTES - 749), &mut 0).is_err());
        assert!(read_text_entity(&entity, 1.0, 1750, 750, &mut 0, &mut (MAX_WORK - 2499)).is_err());
        assert!(read(&basic(&"\\U+4E2D".repeat(251))).is_err());
        assert!(read(&basic(&"A".repeat(251))).is_err());
    }
}
