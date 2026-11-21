use super::*;
use crate::{
    Seg2Geom, Vec2Geom,
    paper::{Group, Item, ObjectKind, Scene},
};
use solidcore_record::{Seg2, Vec2};

fn position() -> Frame {
    Frame {
        standard: Standard::Iso1101_2017Iso5459_2024,
        characteristic: Characteristic::Position,
        feature: Feature::DerivedFeatureOfSize,
        zone: Zone { value_mm: 0.1, shape: ZoneShape::Diameter, material: Some(MaterialModifier::Maximum), projected_height_mm: None },
        datums: ["A", "B", "C"].into_iter().map(|s| DatumReference { identifier: s.into(), material: None }).collect(),
    }
}

fn valid_characteristic(characteristic: Characteristic) -> Frame {
    let mut f = position();
    f.characteristic = characteristic;
    f.zone.material = None;
    f.zone.shape = if characteristic == Characteristic::Concentricity { ZoneShape::Diameter } else { ZoneShape::Width };
    f.feature = if matches!(characteristic, Characteristic::Position | Characteristic::Concentricity | Characteristic::Symmetry) {
        Feature::DerivedFeatureOfSize
    } else {
        Feature::Surface
    };
    if characteristic.is_form() {
        f.datums.clear();
    }
    f
}

fn diagnostic_number(n: f64) -> Result<(String, f64)> {
    Ok((if n == 0.0 { "0".into() } else { n.to_string() }, if n == 0.0 { 0.0 } else { n }))
}

fn assert_bad(f: &Frame) {
    assert!(f.validate().is_err(), "{f:?}");
    assert!(f.text().is_err());
    assert!(f.compartments().is_err());
    assert!(f.formatted_compartments(diagnostic_number).is_err());
    assert!(f.warnings().is_err());
    assert!(f.layout(Style::default(), diagnostic_number).is_err());
}

fn assert_paper(layout: Layout) {
    // This foundation deliberately has no new paper ObjectKind: the caller owns grouping.
    let scene = Scene {
        size_mm: Vec2::new(500.0, 100.0),
        groups: vec![Group { object: 1, kind: ObjectKind::TitleBlock, items: layout.items }],
        fonts: Vec::new(),
        clips: Vec::new(),
    };
    assert_eq!(scene.validate(), Ok(()));
}

#[test]
fn text_has_semantic_compartments_and_datum_order() {
    let mut f = position();
    assert_eq!(f.text().unwrap(), "⌖ | ⌀0.1Ⓜ | A | B | C");
    assert_eq!(f.compartments().unwrap().len(), 5);
    f.datums.swap(0, 2);
    f.datums[0].material = Some(MaterialModifier::Least);
    assert_eq!(f.text().unwrap(), "⌖ | ⌀0.1Ⓜ | CⓁ | B | A");
    assert!(f.warnings().unwrap().is_empty());
}

#[test]
fn datum_free_position_is_accepted_and_reported_for_context_checking() {
    let mut f = position();
    f.datums.clear();
    assert_eq!(f.validate(), Ok(()));
    assert_eq!(f.text().unwrap(), "⌖ | ⌀0.1Ⓜ");
    assert_eq!(f.warnings().unwrap(), vec![Warning::DatumFreePositionNeedsPatternContext]);
    assert_paper(f.layout(Style::default(), diagnostic_number).unwrap());
}

#[test]
fn all_fourteen_iso_characteristics_validate_and_draw() {
    let mut texts = std::collections::HashSet::new();
    for &characteristic in Characteristic::ALL {
        let f = valid_characteristic(characteristic);
        assert_eq!(f.validate(), Ok(()), "{characteristic:?}");
        texts.insert(characteristic.text());
        assert_paper(f.layout(Style::default(), diagnostic_number).unwrap());
    }
    assert_eq!(texts.len(), 14);
}

#[test]
fn asme_2018_has_twelve_characteristics() {
    let mut accepted = 0;
    for &characteristic in Characteristic::ALL {
        let mut f = valid_characteristic(characteristic);
        f.standard = Standard::AsmeY14_5_2018;
        if matches!(characteristic, Characteristic::Concentricity | Characteristic::Symmetry) {
            assert_bad(&f);
        } else {
            assert_eq!(f.validate(), Ok(()));
            accepted += 1;
        }
    }
    assert_eq!(accepted, 12);
}

#[test]
fn datum_count_rules_follow_characteristics() {
    for &characteristic in Characteristic::ALL {
        let mut f = valid_characteristic(characteristic);
        f.datums.clear();
        assert_eq!(f.validate().is_ok(), !characteristic.requires_datums());
        if characteristic.is_form() {
            f.datums.push(DatumReference { identifier: "A".into(), material: None });
            assert_bad(&f);
        }
    }
    let mut f = position();
    f.datums.push(DatumReference { identifier: "D".into(), material: None });
    assert_bad(&f);
    f.datums = vec![DatumReference { identifier: "A".into(), material: None }; 100_000];
    assert_bad(&f);
}

#[test]
fn repeated_datums_and_unsupported_common_datums_are_rejected() {
    let mut f = position();
    f.datums[2].identifier = "A".into();
    assert_bad(&f);
    for s in ["A-B", "A1", "A|B", "AⓂ", "", "A\n", "a", "ABC", "Å", "Ａ", "I", "O", "Q", "AI", "QA"] {
        f.datums[2].identifier = s.into();
        assert_bad(&f);
    }
}

#[test]
fn datum_identifier_alphabet_is_bounded_and_has_original_geometry() {
    for standard in [Standard::Iso1101_2017Iso5459_2024, Standard::AsmeY14_5_2018] {
        for s in ["A", "Z", "AA", "AZ", "ZZ"] {
            let d = DatumIdentifier { standard, letters: s.into() };
            assert_eq!(d.validate(), Ok(()));
            assert_eq!(d.text().unwrap(), format!("[{s}]"));
            assert_paper(d.layout(Style::default()).unwrap());
        }
        for s in ["", "I", "O", "Q", "AI", "a", "A1", "A-B", "ABC", "A\0", "🦀"] {
            let d = DatumIdentifier { standard, letters: s.into() };
            assert!(d.validate().is_err());
            assert!(d.text().is_err());
            assert!(d.layout(Style::default()).is_err());
        }
    }
    for c in b'A'..=b'Z' {
        if !matches!(c, b'I' | b'O' | b'Q') {
            let d = DatumIdentifier { standard: position().standard, letters: char::from(c).to_string() };
            assert_paper(d.layout(Style::default()).unwrap());
        }
    }
}

#[test]
fn all_hostile_numeric_tolerances_are_rejected_at_every_boundary() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, -1e-9, f64::MIN_POSITIVE, 1e-10, 10_000.01, f64::MAX] {
        let mut f = position();
        f.zone.value_mm = value;
        assert_bad(&f);
    }
    for value in [1e-9, 1e-6, 0.123456789, 1.0, 10_000.0] {
        let mut f = position();
        f.zone.value_mm = value;
        assert_eq!(f.validate(), Ok(()));
        assert!(f.text().unwrap().contains(&value.to_string()));
        assert_paper(f.layout(Style::default(), diagnostic_number).unwrap());
    }
}

#[test]
fn zero_is_supported_at_maximum_material_and_normalised() {
    for value in [0.0, -0.0] {
        let mut f = position();
        f.zone.value_mm = value;
        assert_eq!(f.text().unwrap(), "⌖ | ⌀0Ⓜ | A | B | C");
        for modifier in [None, Some(MaterialModifier::Least)] {
            f.zone.material = modifier;
            assert_bad(&f);
        }
    }
}

#[test]
fn feature_and_zone_shape_are_validated_together() {
    use Characteristic::*;
    use Feature::{DerivedFeatureOfSize as D, Surface as S};
    use ZoneShape::{Diameter as Dia, SphericalDiameter as Sphere, Width as W};
    // Explicit supported combinations, independent of the validator's predicates.
    let cases: &[(Characteristic, &[(Feature, ZoneShape)])] = &[
        (Straightness, &[(S, W), (D, W), (D, Dia)]),
        (Flatness, &[(S, W), (D, W)]),
        (Roundness, &[(S, W)]),
        (Cylindricity, &[(S, W)]),
        (LineProfile, &[(S, W)]),
        (SurfaceProfile, &[(S, W)]),
        (Parallelism, &[(S, W), (D, W), (D, Dia)]),
        (Perpendicularity, &[(S, W), (D, W), (D, Dia)]),
        (Angularity, &[(S, W), (D, W), (D, Dia)]),
        (Position, &[(D, W), (D, Dia), (D, Sphere)]),
        (Concentricity, &[(D, Dia)]),
        (Symmetry, &[(D, W)]),
        (CircularRunout, &[(S, W)]),
        (TotalRunout, &[(S, W)]),
    ];
    assert_eq!(cases.len(), Characteristic::ALL.len());
    for &(characteristic, allowed) in cases {
        for feature in [Feature::Surface, Feature::DerivedFeatureOfSize] {
            for shape in [ZoneShape::Width, ZoneShape::Diameter, ZoneShape::SphericalDiameter] {
                let mut f = valid_characteristic(characteristic);
                f.feature = feature;
                f.zone.shape = shape;
                assert_eq!(f.validate().is_ok(), allowed.contains(&(feature, shape)), "{characteristic:?}, {feature:?}, {shape:?}");
                if f.validate().is_ok() {
                    assert_paper(f.layout(Style::default(), diagnostic_number).unwrap());
                }
            }
        }
    }
    let mut f = valid_characteristic(Characteristic::Flatness);
    f.zone.shape = ZoneShape::Diameter;
    assert_bad(&f);
    f = position();
    f.feature = Feature::Surface;
    assert_bad(&f);
    f = position();
    f.zone.shape = ZoneShape::SphericalDiameter;
    assert_eq!(f.text().unwrap(), "⌖ | S⌀0.1Ⓜ | A | B | C");
    f.characteristic = Characteristic::Parallelism;
    assert_bad(&f);
}

#[test]
fn material_modifiers_require_a_supported_size_feature() {
    for characteristic in [
        Characteristic::Straightness,
        Characteristic::Flatness,
        Characteristic::Parallelism,
        Characteristic::Perpendicularity,
        Characteristic::Angularity,
        Characteristic::Position,
    ] {
        let mut f = valid_characteristic(characteristic);
        f.feature = Feature::DerivedFeatureOfSize;
        for material in [MaterialModifier::Maximum, MaterialModifier::Least] {
            f.zone.material = Some(material);
            assert_eq!(f.validate(), Ok(()));
            assert_paper(f.layout(Style::default(), diagnostic_number).unwrap());
        }
        f.feature = Feature::Surface;
        assert_bad(&f);
    }
    for characteristic in [
        Characteristic::Roundness,
        Characteristic::Cylindricity,
        Characteristic::LineProfile,
        Characteristic::SurfaceProfile,
        Characteristic::Concentricity,
        Characteristic::Symmetry,
        Characteristic::CircularRunout,
        Characteristic::TotalRunout,
    ] {
        let mut f = valid_characteristic(characteristic);
        f.zone.material = Some(MaterialModifier::Maximum);
        assert_bad(&f);
    }
}

#[test]
fn projected_height_uses_asme_convention_and_checks_its_value() {
    let mut f = position();
    f.zone.projected_height_mm = Some(12.5);
    assert_bad(&f); // ISO does not put the height in the frame.
    f.standard = Standard::AsmeY14_5_2018;
    assert_eq!(f.text().unwrap(), "⌖ | ⌀0.1ⓂⓅ12.5 | A | B | C");
    assert_paper(f.layout(Style::default(), diagnostic_number).unwrap());
    for value in [0.0, -1.0, 1e-10, f64::NAN, f64::INFINITY, 10001.0] {
        f.zone.projected_height_mm = Some(value);
        assert_bad(&f);
    }
    f.zone.projected_height_mm = Some(1.0);
    f.zone.shape = ZoneShape::SphericalDiameter;
    assert_bad(&f);
    f.zone.shape = ZoneShape::Diameter;
    f.characteristic = Characteristic::Parallelism;
    assert_bad(&f);
}

#[test]
fn serde_preserves_semantics_but_layout_revalidates_untrusted_records() {
    let f = position();
    let encoded = serde_json::to_value(&f).unwrap();
    assert_eq!(encoded["standard"], "iso1101_2017_iso5459_2024");
    assert_eq!(serde_json::from_value::<Frame>(encoded.clone()).unwrap(), f);
    let mut bad = encoded.clone();
    bad["zone"]["value_mm"] = serde_json::json!(-0.1);
    let parsed: Frame = serde_json::from_value(bad).unwrap();
    assert_bad(&parsed);
    for path in ["extra", "zone", "datum"] {
        let mut bad = encoded.clone();
        match path {
            "zone" => bad["zone"]["unknown"] = true.into(),
            "datum" => bad["datums"][0]["unknown"] = true.into(),
            _ => bad["unknown"] = true.into(),
        }
        assert!(serde_json::from_value::<Frame>(bad).is_err());
    }
    for unsupported in ["composite", "tangent_plane", "free_state", "unequally_disposed"] {
        let mut bad = encoded.clone();
        bad["zone"]["material"] = unsupported.into();
        assert!(serde_json::from_value::<Frame>(bad).is_err());
    }
    let d = DatumIdentifier { standard: f.standard, letters: "AB".into() };
    assert_eq!(serde_json::from_str::<DatumIdentifier>(&serde_json::to_string(&d).unwrap()).unwrap(), d);
}

#[test]
fn paper_geometry_has_five_compartments_with_a_square_symbol_cell() {
    let layout = position().layout(Style::default(), diagnostic_number).unwrap();
    let cells = &layout.compartments;
    assert_eq!(cells.len(), 5);
    assert_eq!(cells[0].size(), Vec2::new(7.0, 7.0));
    // h=3.5: ISO ⌀(0.7h), 0(0.6h), dot(0.2h), 1(0.6h), M(1.2h), four 0.2h gaps,
    // plus 0.7 mm ink clearance and 0.875 mm frame/miter allowance on each side = 17.5 mm.
    assert!((cells[1].size().x - 17.5).abs() < 1e-10);
    assert!((cells[2].size().x - 5.25).abs() < 1e-10);
    for pair in cells.windows(2) {
        assert!((pair[0].max.x - pair[1].min.x).abs() < 1e-10);
    }
    assert_eq!(layout.size_mm.y, 7.0);
    assert_eq!(cells.last().unwrap().max, layout.size_mm);
    assert_paper(layout);
}

#[test]
fn datum_cell_clearance_accounts_for_frame_and_letter_ink() {
    for h in [1.8_f64, 2.5, 3.5, 5.0, 20.0] {
        let d = h / 10.0;
        let minimum_gap = (2.0 * d).max(0.7);
        for letters in ["A", "B", "M", "W", "AA", "BW", "MW", "ZZ"] {
            let datum = DatumIdentifier { standard: position().standard, letters: letters.into() };
            let layout = datum.layout(Style { text_height_mm: h }).unwrap();
            // The first item is the outer rectangle. Every following item is lettering.
            // Use the worst permitted miter extension, rather than merely checking centre lines.
            for item in layout.items.iter().skip(1) {
                if let Item::Stroke { path, .. } = item {
                    for seg in path {
                        for t in [0.0, 0.5, 1.0] {
                            let x = seg.point_at(t).x;
                            assert!(x - 2.0 * d - d / 2.0 >= minimum_gap - 1e-9, "{letters}, h={h}");
                            assert!(layout.size_mm.x - d / 2.0 - x - 2.0 * d >= minimum_gap - 1e-9, "{letters}, h={h}");
                        }
                    }
                }
            }
            assert_paper(layout);
        }
    }
}

#[test]
fn characteristic_geometry_contains_exact_circles_and_cross_lines() {
    let layout = position().layout(Style { text_height_mm: 5.0 }, diagnostic_number).unwrap();
    let center = Vec2::new(5.0, 5.0);
    assert!(layout.items.iter().any(|item| match item {
        Item::Stroke { path, closed: true, width_mm, pattern: None } =>
            *width_mm == 0.5
                && path.iter().any(|seg| matches!(seg,
            Seg2::Arc { center: c, radius, sweep, .. } if *c == center && *radius == 2.0 && *sweep == std::f64::consts::TAU)),
        _ => false,
    }));
    assert!(layout.items.iter().any(|item| match item {
        Item::Stroke { path, .. } => path.iter().any(|seg| matches!(seg, Seg2::Line { a, b }
            if *a == Vec2::new(1.75, 5.0) && *b == Vec2::new(8.25, 5.0))),
        _ => false,
    }));
}

#[test]
fn iso_diameter_grid_has_a_six_d_circle_and_three_by_nine_d_diagonal() {
    // The ISO 7083:2021 diameter dimensions were checked independently.
    // h=5, d=0.5. The tolerance text starts at (12.25,2.5); the circle centre is
    // (14,5), radius 3d, diagonal endpoints differ by 3d in x and 9d in y.
    let layout = position().layout(Style { text_height_mm: 5.0 }, diagnostic_number).unwrap();
    assert!(layout.items.iter().any(|item| matches!(item,
        Item::Stroke { path, closed: true, width_mm, pattern: None } if *width_mm == 0.5 && path.iter().any(|seg|
            matches!(seg, Seg2::Arc { center, radius, sweep, .. }
                if *center == Vec2::new(14.0, 5.0) && *radius == 1.5 && *sweep == std::f64::consts::TAU)))));
    let ends = [Vec2::new(13.25, 2.75), Vec2::new(14.75, 7.25)];
    assert!(layout.items.iter().any(|item| matches!(item,
        Item::Stroke { path, width_mm, .. } if *width_mm == 0.5 && path.iter().any(|seg|
            matches!(seg, Seg2::Line { a, b } if *a == ends[0] && *b == ends[1])))));
    for end in ends {
        // An analytic circle fills each d/2 end cap, so painted height is h.
        assert!(layout.items.iter().any(|item| matches!(item,
            Item::Fill { region } if region.holes.is_empty() && !region.outer.segs.is_empty()
                && region.outer.segs.iter().all(|seg| matches!(seg,
                    Seg2::Arc { center, radius, .. } if *center == end && *radius == 0.25))
                && (region.outer.segs.iter().map(|seg| match seg {
                    Seg2::Arc { sweep, .. } => *sweep,
                    _ => 0.0,
                }).sum::<f64>() - std::f64::consts::TAU).abs() < 1e-12)));
    }
    assert_paper(layout);
}

#[test]
fn iso_diameter_proportions_are_not_assigned_to_the_asme_preview() {
    let iso = position();
    let mut asme = iso.clone();
    asme.standard = Standard::AsmeY14_5_2018;
    assert_eq!(iso.text().unwrap(), asme.text().unwrap());
    let iso = iso.layout(Style::default(), diagnostic_number).unwrap();
    let asme = asme.layout(Style::default(), diagnostic_number).unwrap();
    assert!((iso.compartments[1].size().x - 17.5).abs() < 1e-10);
    assert!((asme.compartments[1].size().x - 18.2).abs() < 1e-10);
    assert_eq!(iso.items.iter().filter(|item| matches!(item, Item::Fill { .. })).count(), 2);
    assert_eq!(asme.items.iter().filter(|item| matches!(item, Item::Fill { .. })).count(), 0);
    assert_paper(iso);
    assert_paper(asme);
}

#[test]
fn style_limits_reject_nonfinite_and_too_small_or_large_heights() {
    let d = DatumIdentifier { standard: position().standard, letters: "A".into() };
    for h in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 1.799, 20.001, f64::MAX] {
        let s = Style { text_height_mm: h };
        assert!(s.validate().is_err());
        assert!(position().layout(s, diagnostic_number).is_err());
        assert!(d.layout(s).is_err());
    }
    for h in [1.8, 2.5, 3.5, 5.0, 7.0, 10.0, 14.0, 20.0] {
        assert_paper(position().layout(Style { text_height_mm: h }, diagnostic_number).unwrap());
        let layout = d.layout(Style { text_height_mm: h }).unwrap();
        assert_eq!(layout.size_mm, Vec2::new(2.0 * h, 2.0 * h));
        assert_paper(layout);
    }
}

#[test]
fn geometry_scales_with_lettering_and_is_independent_of_model_scale() {
    // No view scale exists in this API. Above the minimum clearance, all distances scale with h.
    let a = position().layout(Style { text_height_mm: 5.0 }, diagnostic_number).unwrap();
    let b = position().layout(Style { text_height_mm: 10.0 }, diagnostic_number).unwrap();
    assert!((b.size_mm - a.size_mm * 2.0).len() < 1e-10);
    assert_eq!(a.items.len(), b.items.len());
    let assert_scaled_paths = |pa: &[Seg2], pb: &[Seg2]| {
        assert_eq!(pa.len(), pb.len());
        for (sa, sb) in pa.iter().zip(pb) {
            for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
                assert!((sb.point_at(t) - sa.point_at(t) * 2.0).len() < 1e-10);
            }
        }
    };
    for (a, b) in a.items.iter().zip(&b.items) {
        match (a, b) {
            (Item::Stroke { path: pa, width_mm: wa, .. }, Item::Stroke { path: pb, width_mm: wb, .. }) => {
                assert_eq!(*wb, wa * 2.0);
                assert_scaled_paths(pa, pb);
            }
            (Item::Fill { region: a }, Item::Fill { region: b }) => {
                assert_scaled_paths(&a.outer.segs, &b.outer.segs);
                assert_eq!(a.holes.len(), b.holes.len());
                for (a, b) in a.holes.iter().zip(&b.holes) {
                    assert_scaled_paths(&a.segs, &b.segs);
                }
            }
            _ => panic!("paper item kind changes with lettering height"),
        }
    }
}

#[test]
fn maximum_records_remain_small_and_all_ink_is_within_the_declared_margin() {
    let mut f = position();
    f.standard = Standard::AsmeY14_5_2018;
    f.zone.value_mm = 0.12345678912345678;
    f.zone.projected_height_mm = Some(9999.999999999998);
    for (datum, name) in f.datums.iter_mut().zip(["WW", "MM", "ZZ"]) {
        datum.identifier = name.into();
        datum.material = Some(MaterialModifier::Maximum);
    }
    let layout = f.layout(Style { text_height_mm: 20.0 }, diagnostic_number).unwrap();
    assert!(layout.items.len() < 300);
    let bounds = crate::Aabb2 { min: Vec2::ZERO, max: layout.size_mm }.inflated(1.0);
    for item in &layout.items {
        if let Item::Stroke { path, .. } = item {
            for seg in path {
                for i in 0..=20 {
                    assert!(bounds.contains(seg.point_at(f64::from(i) / 20.0)));
                }
            }
        }
    }
    assert_paper(layout);
}

#[test]
fn provisional_characteristics_do_not_cross_the_frame_ink() {
    // Calibration must preserve containment, without blessing any guessed proportions.
    for standard in [Standard::Iso1101_2017Iso5459_2024, Standard::AsmeY14_5_2018] {
        for h in [1.8_f64, 3.5, 20.0] {
            for &characteristic in Characteristic::ALL {
                let mut frame = valid_characteristic(characteristic);
                frame.standard = standard;
                if frame.validate().is_err() {
                    continue; // Concentricity/symmetry are not in the supported ASME edition.
                }
                let layout = frame.layout(Style { text_height_mm: h }, diagnostic_number).unwrap();
                let mut checked = 0;
                for item in &layout.items {
                    if let Item::Stroke { path, .. } = item
                        && matches!(path.as_slice(), [Seg2::Line { a, b }]
                            if *a == Vec2::new(2.0 * h, 0.0) && *b == Vec2::new(2.0 * h, 2.0 * h))
                    {
                        break; // End of the characteristic, before the first divider.
                    }
                    let (path, allowance) = match item {
                        Item::Stroke { path, width_mm, .. } => (path.as_slice(), 2.0 * width_mm),
                        Item::Fill { region } => (region.outer.segs.as_slice(), 0.0),
                        Item::Text(_) | Item::Picture(_) => panic!("GD&T symbols use original vector strokes and fills only"),
                    };
                    // A stroke's worst miter reach plus the frame's half-width.
                    let margin = allowance + h / 20.0;
                    let inside = crate::Aabb2 { min: Vec2::new(margin, margin), max: Vec2::new(2.0 * h - margin, 2.0 * h - margin) }.inflated(1e-9);
                    for seg in path {
                        for step in 0..=32 {
                            assert!(inside.contains(seg.point_at(f64::from(step) / 32.0)), "{standard:?}/{characteristic:?}, h={h}");
                        }
                    }
                    checked += 1;
                }
                assert!(checked > 0);
                assert_paper(layout);
            }
        }
    }
}

#[test]
fn formatted_layout_uses_caller_decimal_marker_and_precision() {
    let mut f = position();
    f.zone.value_mm = 0.1;
    let format_comma = |n: f64| Ok((format!("{n:.2}").replace('.', ","), (n * 100.0).round() / 100.0));
    let compartments = f.formatted_compartments(format_comma).unwrap();
    assert_eq!(compartments, vec!["⌖", "⌀0,10Ⓜ", "A", "B", "C"]);
    let layout = f.layout(Style::default(), format_comma).unwrap();
    assert_paper(layout);

    // text() and compartments() remain lossless.
    assert_eq!(f.text().unwrap(), "⌖ | ⌀0.1Ⓜ | A | B | C");
    assert_eq!(f.compartments().unwrap(), vec!["⌖", "⌀0.1Ⓜ", "A", "B", "C"]);
}

#[test]
fn formatted_layout_supports_grouping_with_narrow_no_break_space() {
    let mut f = position();
    f.zone.value_mm = 1234.5;
    let format_grouped = |n: f64| {
        let s = format!("{n:.1}").replace('.', ",");
        let grouped = if s.len() > 5 { format!("{}\u{202f}{}", &s[..1], &s[1..]) } else { s };
        Ok((grouped, n))
    };
    let compartments = f.formatted_compartments(format_grouped).unwrap();
    assert_eq!(compartments[1], "⌀1\u{202f}234,5Ⓜ");
    let layout = f.layout(Style::default(), format_grouped).unwrap();
    assert_paper(layout);
}

#[test]
fn positive_tolerance_rounding_to_zero_is_rejected_while_explicit_zero_is_retained() {
    let format_zero = |_| Ok(("0".into(), 0.0));
    let mut f = position();
    f.zone.value_mm = 0.001;
    assert!(f.formatted_compartments(format_zero).is_err());
    assert!(f.layout(Style::default(), format_zero).is_err());

    // Explicit zero tolerance is permitted at maximum material.
    f.zone.value_mm = 0.0;
    f.zone.material = Some(MaterialModifier::Maximum);
    assert_eq!(f.validate(), Ok(()));
    let compartments = f.formatted_compartments(format_zero).unwrap();
    assert_eq!(compartments[1], "⌀0Ⓜ");
    assert_paper(f.layout(Style::default(), format_zero).unwrap());

    // Formatter specifying precision or comma on explicit zero is retained.
    let format_comma_zero = |_| Ok(("0,00".into(), 0.0));
    let compartments = f.formatted_compartments(format_comma_zero).unwrap();
    assert_eq!(compartments[1], "⌀0,00Ⓜ");
    assert_paper(f.layout(Style::default(), format_comma_zero).unwrap());
}

#[test]
fn projected_height_formatting_and_zero_rounding_rejection() {
    let mut f = position();
    f.standard = Standard::AsmeY14_5_2018;
    f.zone.value_mm = 0.2;
    f.zone.projected_height_mm = Some(12.5);
    let format_comma = |n: f64| Ok((format!("{n:.1}").replace('.', ","), n));
    let compartments = f.formatted_compartments(format_comma).unwrap();
    assert_eq!(compartments[1], "⌀0,2ⓂⓅ12,5");
    assert_paper(f.layout(Style::default(), format_comma).unwrap());

    // Lossless text preserves default representation.
    assert_eq!(f.text().unwrap(), "⌖ | ⌀0.2ⓂⓅ12.5 | A | B | C");

    // Projected height rounding to zero is rejected.
    let format_zero_height = |n: f64| {
        if n > 1.0 { Ok(("0".into(), 0.0)) } else { Ok((n.to_string(), n)) }
    };
    assert!(f.formatted_compartments(format_zero_height).is_err());
    assert!(f.layout(Style::default(), format_zero_height).is_err());
}

#[test]
fn comma_and_narrow_space_glyphs_render_valid_paper_geometry() {
    assert_eq!(geometry::provisional::glyph_width(','), Ok(0.2));
    assert_eq!(geometry::provisional::glyph_width('\u{202f}'), Ok(0.2));
    assert_eq!(geometry::provisional::glyph_width('.'), Ok(0.2));

    let mut f = position();
    f.zone.value_mm = 12.34;
    let format_comma_grouped = |_| Ok(("12\u{202f}345,67".into(), 12345.67));
    let layout = f.layout(Style::default(), format_comma_grouped).unwrap();
    assert_paper(layout);
}

#[test]
fn invalid_formatted_number_bounds_and_characters_are_rejected() {
    let long_text = format!("0.{}", "0".repeat(127)); // 129 bytes
    assert!(position().formatted_compartments(|n| Ok((long_text.clone(), n))).is_err());
    assert!(position().layout(Style::default(), |n| Ok((long_text.clone(), n))).is_err());

    assert!(position().formatted_compartments(|n| Ok(("".into(), n))).is_err());
    assert!(position().layout(Style::default(), |n| Ok(("".into(), n))).is_err());

    assert!(position().layout(Style::default(), |n| Ok(("0#1".into(), n))).is_err());

    assert!(position().formatted_compartments(|_| Err(Invalid("format error"))).is_err());
    assert!(position().layout(Style::default(), |_| Err(Invalid("format error"))).is_err());
}
