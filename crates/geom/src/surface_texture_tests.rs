use super::*;
use crate::paper::{Group, ObjectKind, Scene};

fn diagnostic_number(n: f64) -> Result<(String, f64)> {
    Ok((n.to_string(), n))
}

fn iso() -> Indication {
    Indication {
        specification: Specification::Iso21920_1_2021 {
            lines: vec![IsoParameterLine {
                parameter: IsoParameter::Ra,
                tolerance: ToleranceType::Upper,
                limit_um: 1.6,
                acceptance: AcceptanceRule::Maximum,
            }],
        },
        kind: SymbolKind::RemovalRequired,
        lay: Some(Lay::Perpendicular),
        process: Some("ground".into()),
        all_round: true,
    }
}

fn asme(unit: AmplitudeUnit) -> Indication {
    Indication {
        specification: Specification::AsmeY14_36_2018 {
            unit,
            lines: vec![AsmeParameterLine { parameter: AsmeParameter::Ra, maximum: 63.0, minimum: Some(32.0) }],
        },
        ..iso()
    }
}

fn iso_lines(indication: &mut Indication) -> &mut Vec<IsoParameterLine> {
    match &mut indication.specification {
        Specification::Iso21920_1_2021 { lines } => lines,
        _ => panic!("ISO fixture required"),
    }
}

fn outputs_reject(indication: &Indication) {
    assert!(indication.validate().is_err(), "{indication:?}");
    assert!(indication.text().is_err());
    assert!(indication.parameter_texts().is_err());
    assert!(indication.preview_geometry(3.5, 30.0, diagnostic_number).is_err());
}

#[test]
fn iso_text_preserves_explicit_rule_and_parameter_order() {
    let mut indication = iso();
    iso_lines(&mut indication).push(IsoParameterLine {
        parameter: IsoParameter::Rz,
        tolerance: ToleranceType::Lower,
        limit_um: 4.0,
        acceptance: AcceptanceRule::SixteenPercent,
    });
    iso_lines(&mut indication).push(IsoParameterLine {
        parameter: IsoParameter::Rq,
        tolerance: ToleranceType::Upper,
        limit_um: 2.0,
        acceptance: AcceptanceRule::Median,
    });
    assert_eq!(indication.parameter_texts().unwrap(), ["Ra 1.6", "L Rz 4 T16%", "Rq 2 Tmed"]);
    assert_eq!(
        indication.text().unwrap(),
        "ISO 21920-1:2021; um; default rule=maximum; material removal required; Ra 1.6 / L Rz 4 T16% / Rq 2 Tmed; lay=perpendicular; process=\"ground\"; all-round"
    );
    let json = serde_json::to_value(&indication).unwrap();
    assert_eq!(json["specification"]["standard"], "iso_21920_1_2021");
    assert_eq!(json["specification"]["lines"][0]["acceptance"], "maximum");
    assert_eq!(serde_json::from_value::<Indication>(json).unwrap(), indication);
}

#[test]
fn asme_units_are_explicit_and_values_are_never_silently_converted() {
    let metric = asme(AmplitudeUnit::Micrometre);
    let inch = asme(AmplitudeUnit::Microinch);
    assert_eq!(metric.parameter_texts().unwrap(), ["Ra 32-63"]);
    assert_eq!(inch.parameter_texts().unwrap(), ["Ra 32-63"]);
    assert!(metric.text().unwrap().starts_with("ASME Y14.36-2018; um;"));
    assert!(inch.text().unwrap().starts_with("ASME Y14.36-2018; uin;"));
    assert!(!inch.text().unwrap().contains("default rule"));
    assert_eq!(serde_json::from_str::<Indication>(&serde_json::to_string(&inch).unwrap()).unwrap(), inch);
}

#[test]
fn paired_iso_limits_accept_equality_and_either_order_but_not_contradictions() {
    for reverse in [false, true] {
        let mut indication = iso();
        let mut lower = iso_lines(&mut indication)[0].clone();
        lower.tolerance = ToleranceType::Lower;
        iso_lines(&mut indication).push(lower);
        if reverse {
            iso_lines(&mut indication).reverse();
        }
        indication.validate().unwrap();
        let lower = iso_lines(&mut indication).iter_mut().find(|p| p.tolerance == ToleranceType::Lower).unwrap();
        lower.limit_um = 1.7;
        outputs_reject(&indication);
    }
    let mut indication = iso();
    let mut lower = iso_lines(&mut indication)[0].clone();
    lower.tolerance = ToleranceType::Lower;
    lower.acceptance = AcceptanceRule::SixteenPercent;
    iso_lines(&mut indication).push(lower);
    outputs_reject(&indication);
    let mut duplicate = iso();
    let copy = iso_lines(&mut duplicate)[0].clone();
    iso_lines(&mut duplicate).push(copy);
    outputs_reject(&duplicate);
}

#[test]
fn hostile_numbers_fail_all_output_boundaries() {
    for n in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 1e6 + 1.0] {
        let mut indication = iso();
        iso_lines(&mut indication)[0].limit_um = n;
        outputs_reject(&indication);
        for minimum in [true, false] {
            let mut indication = asme(AmplitudeUnit::Microinch);
            if let Specification::AsmeY14_36_2018 { lines, .. } = &mut indication.specification {
                if minimum {
                    lines[0].minimum = Some(n);
                } else {
                    lines[0].maximum = n;
                }
            }
            outputs_reject(&indication);
        }
    }
    for n in [f64::MIN_POSITIVE, 1e6] {
        let mut indication = iso();
        iso_lines(&mut indication)[0].limit_um = n;
        indication.validate().unwrap();
    }
}

#[test]
fn asme_ranges_and_duplicate_parameters_are_checked() {
    let mut indication = asme(AmplitudeUnit::Micrometre);
    if let Specification::AsmeY14_36_2018 { lines, .. } = &mut indication.specification {
        lines[0].minimum = Some(64.0);
    }
    outputs_reject(&indication);
    if let Specification::AsmeY14_36_2018 { lines, .. } = &mut indication.specification {
        lines[0].minimum = Some(63.0);
    }
    indication.validate().unwrap();
    if let Specification::AsmeY14_36_2018 { lines, .. } = &mut indication.specification {
        lines.push(lines[0].clone());
    }
    outputs_reject(&indication);
}

#[test]
fn empty_and_excessive_collections_are_rejected_before_geometry() {
    for specification in
        [Specification::Iso21920_1_2021 { lines: vec![] }, Specification::AsmeY14_36_2018 { unit: AmplitudeUnit::Micrometre, lines: vec![] }]
    {
        outputs_reject(&Indication { specification, ..iso() });
    }
    let mut indication = iso();
    let line = iso_lines(&mut indication)[0].clone();
    *iso_lines(&mut indication) = vec![line; MAX_PARAMETER_LINES + 1];
    outputs_reject(&indication);
    // All eight ISO parameters may have one lower and one upper limit: exactly sixteen lines.
    let mut indication = iso();
    *iso_lines(&mut indication) = [
        IsoParameter::Ra,
        IsoParameter::Rz,
        IsoParameter::Rp,
        IsoParameter::Rv,
        IsoParameter::Rq,
        IsoParameter::Rmax,
        IsoParameter::Rt,
        IsoParameter::Pt,
    ]
    .into_iter()
    .flat_map(|parameter| {
        [ToleranceType::Upper, ToleranceType::Lower].map(|tolerance| IsoParameterLine {
            parameter,
            tolerance,
            limit_um: 1.0,
            acceptance: AcceptanceRule::Maximum,
        })
    })
    .collect();
    indication.validate().unwrap();
    let geometry = indication.preview_geometry(20.0, 1000.0, diagnostic_number).unwrap();
    assert_eq!(geometry.parameters.len(), MAX_PARAMETER_LINES);
    assert!(geometry.parameters.windows(2).all(|p| (p[0].baseline_mm.y - p[1].baseline_mm.y - 30.0).abs() < 1e-9));
    assert!(geometry.parameters.last().unwrap().baseline_mm.y >= 0.0);
}

#[test]
fn process_limits_count_utf8_bytes_and_do_not_hide_control_characters() {
    for process in [
        "",
        " ground",
        "ground ",
        "\n",
        "a\rb",
        "a\tb",
        "a\0b",
        "a\u{202e}b",
        "a\u{2066}b",
        "a\u{200b}b",
        "a\u{00a0}b",
        "a\u{2028}b",
        "a\u{180e}b",
        "a\u{e0001}b",
    ] {
        outputs_reject(&Indication { process: Some(process.into()), ..iso() });
    }
    let mut indication = Indication { process: Some("é".repeat(MAX_PROCESS_BYTES / 2)), ..iso() };
    indication.validate().unwrap();
    indication.process.as_mut().unwrap().push('é');
    outputs_reject(&indication);
    let indication = Indication { process: Some("ground; then \"polished\"".into()), ..iso() };
    assert!(indication.text().unwrap().contains("process=\"ground; then \\\"polished\\\"\""));
}

#[test]
fn serde_does_not_accept_unknown_fields_legacy_families_or_missing_rules() {
    for pointer in ["", "/specification", "/specification/lines/0"] {
        let mut json = serde_json::to_value(iso()).unwrap();
        json.pointer_mut(pointer).unwrap().as_object_mut().unwrap().insert("surprise".into(), true.into());
        assert!(serde_json::from_value::<Indication>(json).is_err());
    }
    let mut json = serde_json::to_value(iso()).unwrap();
    json["specification"]["lines"][0].as_object_mut().unwrap().remove("acceptance");
    assert!(serde_json::from_value::<Indication>(json).is_err());
    for standard in ["iso1302_2002", "iso_1302_2002", "future_standard"] {
        let mut json = serde_json::to_value(iso()).unwrap();
        json["specification"]["standard"] = standard.into();
        assert!(serde_json::from_value::<Indication>(json).is_err());
    }
    let mut json = serde_json::to_value(iso()).unwrap();
    json["specification"]["unit"] = "microinch".into();
    assert!(serde_json::from_value::<Indication>(json).is_err());
    let mut json = serde_json::to_value(asme(AmplitudeUnit::Microinch)).unwrap();
    json["specification"]["lines"][0]["acceptance"] = "maximum".into();
    assert!(serde_json::from_value::<Indication>(json).is_err());
}

fn path(item: &Item) -> &[Seg2] {
    match item {
        Item::Stroke { path, .. } => path,
        _ => panic!("expected stroke"),
    }
}

#[test]
fn revised_preview_uses_nominal_iso_height_and_all_round_corner() {
    for (indication, short) in [(iso(), 7.0), (asme(AmplitudeUnit::Micrometre), 7.5)] {
        let geometry = indication.preview_geometry(5.0, 30.0, diagnostic_number).unwrap();
        assert_eq!(path(&geometry.strokes[0])[0].start().y, short);
        let corner = path(&geometry.strokes[0])[1].end();
        assert!(matches!(path(&geometry.all_round_mark[0])[0], Seg2::Arc { center, .. } if center == corner));
    }
}

#[test]
fn preview_has_sixty_degree_legs_and_distinguishes_iso_bar_from_removal_bar() {
    let mut indication = iso();
    indication.lay = None;
    indication.kind = SymbolKind::AnyProcess;
    let iso_geometry = indication.preview_geometry(3.5, 30.0, diagnostic_number).unwrap();
    assert_eq!(iso_geometry.strokes.len(), 2);
    assert_eq!(path(&iso_geometry.strokes[0]).len(), 3);
    let (left, tip) = match path(&iso_geometry.strokes[0])[0] {
        Seg2::Line { a, b } => (a, b),
        _ => panic!(),
    };
    assert_eq!(tip, Vec2::ZERO);
    let (_, right) = match path(&iso_geometry.strokes[0])[1] {
        Seg2::Line { a, b } => (a, b),
        _ => panic!(),
    };
    let cos = (left.x * right.x + left.y * right.y) / (left.len() * right.len());
    assert!((cos - 0.5).abs() < 1e-12);
    indication.kind = SymbolKind::RemovalRequired;
    let required = indication.preview_geometry(3.5, 30.0, diagnostic_number).unwrap();
    assert_eq!(required.strokes.len(), 3);
    assert!(matches!(path(&required.strokes[1])[0], Seg2::Line { a, b } if a.y == 5.0 && a.y == b.y));
    indication.specification = asme(AmplitudeUnit::Micrometre).specification;
    indication.kind = SymbolKind::AnyProcess;
    let asme_geometry = indication.preview_geometry(3.5, 30.0, diagnostic_number).unwrap();
    assert_eq!(path(&asme_geometry.strokes[0])[0].start().y, 5.25);
    assert_eq!(path(&iso_geometry.strokes[0])[0].start().y, 5.0);
    assert_eq!(path(&asme_geometry.strokes[0])[1].end(), right);
    // The ISO identifier is detached over the left leg; it is not a second shelf.
    assert!(matches!(path(&iso_geometry.strokes[1])[0], Seg2::Line { a, b } if a.x < 0.0 && b.x > 0.0 && a.y < right.y && a.y == b.y));
}

#[test]
fn prohibited_circle_is_tangent_to_both_legs_and_all_round_is_at_the_corner() {
    let indication = Indication { kind: SymbolKind::RemovalProhibited, lay: None, ..iso() };
    let geometry = indication.preview_geometry(5.0, 30.0, diagnostic_number).unwrap();
    let (center, radius) = match path(geometry.strokes.last().unwrap())[0] {
        Seg2::Arc { center, radius, sweep, .. } => {
            assert_eq!(sweep, TAU);
            (center, radius)
        }
        _ => panic!(),
    };
    assert!((center.y * 0.5 - radius).abs() < 1e-12);
    assert_eq!(geometry.all_round_mark.len(), 1);
    let corner = path(&geometry.strokes[0])[1].end();
    assert!(matches!(path(&geometry.all_round_mark[0])[0], Seg2::Arc { center, radius, .. } if center == corner && radius == 2.0));
    assert!(corner.dist(center) > 2.0 * radius);
    let plain = Indication { all_round: false, ..indication }.preview_geometry(5.0, 30.0, diagnostic_number).unwrap();
    assert_eq!(plain.strokes, geometry.strokes);
    assert!(plain.all_round_mark.is_empty());
}

#[test]
fn all_symbol_and_lay_combinations_produce_valid_paper_strokes_at_nominal_sizes() {
    for kind in [SymbolKind::AnyProcess, SymbolKind::RemovalRequired, SymbolKind::RemovalProhibited] {
        for lay in [
            None,
            Some(Lay::Parallel),
            Some(Lay::Perpendicular),
            Some(Lay::Crossed),
            Some(Lay::Multidirectional),
            Some(Lay::Circular),
            Some(Lay::Radial),
            Some(Lay::Particulate),
        ] {
            for h in [2.5, 3.5, 5.0, 7.0, 10.0, 14.0, 20.0] {
                for specification in [iso().specification, asme(AmplitudeUnit::Microinch).specification] {
                    let indication = Indication { kind, lay, specification, ..iso() };
                    let before = indication.clone();
                    let geometry = indication.preview_geometry(h, 1000.0, diagnostic_number).unwrap();
                    assert_eq!(indication, before);
                    assert_eq!(geometry.parameters[0].height_mm, h);
                    assert!(geometry.process.is_some());
                    let scene = Scene {
                        size_mm: Vec2::new(1000.0, 1000.0),
                        groups: vec![Group {
                            object: 0,
                            kind: ObjectKind::TitleBlock,
                            items: geometry.strokes.into_iter().chain(geometry.all_round_mark).collect(),
                        }],
                    };
                    scene.validate().unwrap();
                }
            }
        }
    }
}

#[test]
fn geometry_rejects_nonfinite_and_unbounded_layout_requests() {
    for h in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 2.499, 20.001, f64::MAX] {
        assert!(iso().preview_geometry(h, 30.0, diagnostic_number).is_err());
        assert!(asme(AmplitudeUnit::Micrometre).preview_geometry(h, 30.0, diagnostic_number).is_err());
    }
    for width in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 3.499, 1000.001, f64::MAX] {
        assert!(iso().preview_geometry(3.5, width, diagnostic_number).is_err());
        assert!(asme(AmplitudeUnit::Microinch).preview_geometry(3.5, width, diagnostic_number).is_err());
    }
}

#[test]
fn preview_scales_in_paper_units_and_text_slots_preserve_the_original_content() {
    for (indication, h) in [(iso(), 5.0), (asme(AmplitudeUnit::Micrometre), 2.5)] {
        let small = indication.preview_geometry(h, 25.0, diagnostic_number).unwrap();
        let large = indication.preview_geometry(2.0 * h, 50.0, diagnostic_number).unwrap();
        for (a, b) in small.strokes.iter().chain(&small.all_round_mark).zip(large.strokes.iter().chain(&large.all_round_mark)) {
            assert_eq!(path(a).len(), path(b).len());
            for (a, b) in path(a).iter().zip(path(b)) {
                assert!(a.start().dist(b.start() * 0.5) < 1e-12);
                assert!(a.end().dist(b.end() * 0.5) < 1e-12);
            }
            if let (Item::Stroke { width_mm: x, .. }, Item::Stroke { width_mm: y, .. }) = (a, b) {
                assert_eq!(*x * 2.0, *y);
            }
        }
        assert_eq!(small.parameters[0].text, indication.parameter_texts().unwrap()[0]);
        assert_eq!(small.process.as_ref().unwrap().text, indication.process.as_ref().unwrap().as_str());
        assert!(small.parameters[0].baseline_mm.dist(large.parameters[0].baseline_mm * 0.5) < 1e-12);
        assert_eq!(small.parameters[0].height_mm * 2.0, large.parameters[0].height_mm);
    }
}

#[test]
fn iso_nominal_dimensions_use_rounding_and_asme_keeps_continuous_sizing() {
    for h in [2.75, 3.0, 3.5 + 1e-12, 4.0, 6.0, 8.0, 19.999] {
        assert!(iso().preview_geometry(h, 30.0, diagnostic_number).is_err(), "ISO height {h}");
        asme(AmplitudeUnit::Micrometre).preview_geometry(h, 30.0, diagnostic_number).unwrap();
    }
    // Independently check representative nominal constructions, not a multiplier:
    // at 5mm, the short endpoint is 7mm, the identifier is 6mm long at y=10.5mm.
    let geometry = Indication { lay: None, kind: SymbolKind::AnyProcess, ..iso() }.preview_geometry(5.0, 30.0, diagnostic_number).unwrap();
    assert_eq!(path(&geometry.strokes[0])[0].start().y, 7.0);
    assert_eq!(path(&geometry.strokes[0])[1].end().y, 15.0);
    assert!(matches!(path(&geometry.strokes[1])[0], Seg2::Line { a, b } if a == Vec2::new(-3.0, 10.5) && b == Vec2::new(3.0, 10.5)));
    assert!(matches!(geometry.strokes[0], Item::Stroke { width_mm, .. } if width_mm == 0.5));
    // The identifier gap rounds from 2mm to 3.5mm across these two sizes: it does
    // not double. Universal linear scaling would silently discard the nominal rule.
    let small = Indication { lay: None, kind: SymbolKind::AnyProcess, ..iso() }.preview_geometry(2.5, 30.0, diagnostic_number).unwrap();
    let small_gap = path(&small.strokes[1])[0].start().y - path(&small.strokes[0])[0].start().y;
    let large_gap = path(&geometry.strokes[1])[0].start().y - path(&geometry.strokes[0])[0].start().y;
    assert_eq!(small_gap, 2.0);
    assert_eq!(large_gap, 3.5);
    let largest = iso().preview_geometry(20.0, 30.0, diagnostic_number).unwrap();
    assert_eq!(path(&largest.strokes[0])[0].start().y, 28.0);
}

#[test]
fn parameter_lines_and_lay_fit_inside_the_reference_bar_at_collection_limits() {
    let mut indication = iso();
    *iso_lines(&mut indication) = [
        IsoParameter::Ra,
        IsoParameter::Rz,
        IsoParameter::Rp,
        IsoParameter::Rv,
        IsoParameter::Rq,
        IsoParameter::Rmax,
        IsoParameter::Rt,
        IsoParameter::Pt,
    ]
    .into_iter()
    .flat_map(|parameter| {
        [
            IsoParameterLine { parameter, tolerance: ToleranceType::Upper, limit_um: 2.0, acceptance: AcceptanceRule::Maximum },
            IsoParameterLine { parameter, tolerance: ToleranceType::Lower, limit_um: 1.0, acceptance: AcceptanceRule::Maximum },
        ]
    })
    .collect();
    for specification in [indication.specification.clone(), asme(AmplitudeUnit::Microinch).specification] {
        for lay in [None, Some(Lay::Circular), Some(Lay::Particulate)] {
            for h in [2.5, 20.0] {
                let geometry =
                    Indication { specification: specification.clone(), lay, ..indication.clone() }.preview_geometry(h, h, diagnostic_number).unwrap();
                let bar = path(&geometry.strokes[0])[2];
                for run in &geometry.parameters {
                    assert!(run.baseline_mm.y >= 0.0 && run.baseline_mm.y + run.height_mm <= bar.start().y);
                    assert!(run.baseline_mm.x > bar.start().x && run.baseline_mm.x + h <= bar.end().x);
                }
                if lay.is_some() {
                    for seg in path(geometry.strokes.last().unwrap()) {
                        for step in 0..=16 {
                            assert!(seg.point_at(step as f64 / 16.0).x < bar.end().x);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn iso_lay_is_above_the_shelf_with_process_while_asme_has_its_own_lower_lay_area() {
    let iso = iso().preview_geometry(5.0, 30.0, diagnostic_number).unwrap();
    let asme = asme(AmplitudeUnit::Micrometre).preview_geometry(5.0, 30.0, diagnostic_number).unwrap();
    let iso_lay = path(iso.strokes.last().unwrap())[0];
    let asme_lay = path(asme.strokes.last().unwrap())[0];
    assert_eq!(iso_lay.start().y, 20.0);
    assert_eq!(asme_lay.start().y, 7.5);
    assert_eq!(iso.process.unwrap().baseline_mm.y, 20.0);
}

#[test]
fn preview_uses_caller_number_policy_for_all_limits() {
    let format_comma = |n: f64| Ok((format!("{n:.2}").replace('.', ","), n));
    let indication = iso();
    let measured = indication.formatted_parameter_texts(format_comma).unwrap();
    let geometry = indication.preview_geometry(3.5, 30.0, format_comma).unwrap();
    assert_eq!(measured, ["Ra 1,60"]);
    assert_eq!(geometry.parameters[0].text, measured[0]);
    let geometry = asme(AmplitudeUnit::Microinch).preview_geometry(3.5, 30.0, format_comma).unwrap();
    assert_eq!(geometry.parameters[0].text, "Ra 32,00-63,00");
    let trim = |n: f64| Ok((format!("{n:.2}").trim_end_matches('0').trim_end_matches('.').to_owned(), n));
    assert_eq!(iso().preview_geometry(3.5, 30.0, trim).unwrap().parameters[0].text, "Ra 1.6");
}

#[test]
fn preview_refuses_zero_rounded_or_unbounded_limits_without_rejecting_record() {
    let mut indication = iso();
    iso_lines(&mut indication)[0].limit_um = f64::MIN_POSITIVE;
    indication.validate().unwrap();
    assert_eq!(indication.parameter_texts().unwrap()[0].len(), 329);
    assert!(indication.preview_geometry(3.5, 30.0, diagnostic_number).is_err());
    assert!(indication.preview_geometry(3.5, 30.0, |_| Ok(("0,00".into(), 0.0))).is_err());
    assert!(indication.preview_geometry(3.5, 30.0, |_| Err(Invalid("formatter error"))).is_err());
    assert!(indication.preview_geometry(3.5, 30.0, |n| Ok((format!("1\u{202e}{n}"), n))).is_err());
    assert!(indication.preview_geometry(3.5, 30.0, |n| Ok((format!("1\u{2028}{n}"), n))).is_err());
    // A caller can provide a compact representation only when its chosen policy allows it.
    assert_eq!(indication.preview_geometry(3.5, 30.0, |n| Ok((format!("{n:e}"), n))).unwrap().parameters[0].text, "Ra 2.2250738585072014e-308");
}

#[test]
fn plain_annotation_policy_rejects_invisible_controls_and_preserves_unicode() {
    for text in ["x\u{200f}y", "x\u{feff}y", "x\u{202e}y", "x\ny"] {
        assert!(!crate::plain_text::is_plain_line(text));
        assert!(Indication { process: Some(text.into()), ..iso() }.validate().is_err());
    }
    for text in ["磨削 café", "polissage α", "溶接"] {
        assert!(crate::plain_text::is_plain_line(text));
        Indication { process: Some(text.into()), ..iso() }.validate().unwrap();
    }
}

#[test]
fn parameter_names_are_explicit_and_stable() {
    assert_eq!(IsoParameter::Rmax.name(), "Rmax");
    assert_eq!(AsmeParameter::Ra.name(), "Ra");
}
