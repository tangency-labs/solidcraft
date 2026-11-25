use super::*;
use crate::shop_rules::{Profile, profile};

/// A 2 mm bracket: one round hole 30 mm from every edge and 20 mm from a bend of radius 2 mm.
fn bracket() -> Part {
    Part {
        thickness: 2.0,
        flat_size: [100.0, 60.0],
        bodies: vec!["Body1".into()],
        holes: vec![hole([50.0, 30.0], 6.0, 30.0, 20.0)],
        ..Part::default()
    }
}

fn hole(at: [f64; 2], diameter: f64, edge: f64, from_bend: f64) -> Hole {
    let gap = BendGap { number: 1, radius: 2.0, inside: false, from_start: from_bend, from_line: from_bend + 2.0 };
    Hole { at, diameter, edge, bends: vec![gap] }
}

fn run(shop: &str, part: &Part) -> Report {
    check(profile(shop).unwrap(), part)
}

fn rule<'a>(r: &'a Report, id: &str) -> Vec<&'a Finding> {
    r.findings.iter().filter(|f| f.rule == id).collect()
}

#[test]
fn a_clean_bracket_passes_generic() {
    // Needs for T = 2, R = 2: a hole at least max(0.5 T, 1) = 1 mm wide, 2 T = 4 mm from an edge
    // (the larger of 1.5 T and 2 T), and max(2 T, 2 T + R) = 6 mm from a bend. The hole is 6 mm
    // wide, 30 mm from the edges and 20 mm from the bend.
    let r = run("generic", &bracket());
    assert!(r.findings.is_empty(), "{:?}", r.findings);
    assert!(r.checked.contains(&"hole.to_bend".to_string()));
    // No bend, no flange: the rule is looked at and nothing is found.
    assert!(r.checked.contains(&"flange.min".to_string()) && !r.not_checked.iter().any(|n| n.rule == "flange.min"));
    let stock = r.not_checked.iter().find(|n| n.rule == "stock.thickness").map(|n| n.why.as_str()).unwrap_or("");
    assert!(stock.contains("Generic's pages give no value"), "{stock}");
}

#[test]
fn a_small_hole_is_a_finding_with_the_shops_limit() {
    let mut part = bracket();
    part.holes.push(hole([12.0, 8.0], 0.6, 8.0, 20.0));
    let f = rule(&run("generic", &part), "hole.min_diameter").into_iter().cloned().collect::<Vec<_>>();
    assert_eq!(f.len(), 1);
    // max(0.5 * 2, 1) = 1 mm.
    assert_eq!((f[0].value, f[0].limit, f[0].severity, f[0].at), (Some(0.6), Some(1.0), Severity::Error, Some([12.0, 8.0])));
    assert_eq!(f[0].message, "the hole at (12, 8) is \u{d8}0.6 mm; Generic's laser cutting needs at least \u{d8}1 mm for 2 mm sheet");
    assert!(f[0].sources.iter().all(|u| u.starts_with("https://")) && !f[0].sources.is_empty());
    let pcb = run("pcbway", &part);
    // PCBWay: half the thickness, 1 mm.
    assert_eq!(rule(&pcb, "hole.min_diameter")[0].limit, Some(1.0));
    assert!(rule(&pcb, "hole.min_diameter")[0].message.starts_with("the hole at (12, 8) is \u{d8}0.6 mm; PCBWay's laser cutting needs"));
}

#[test]
fn a_hole_near_a_bend_is_a_warning_where_the_pages_disagree() {
    let mut part = bracket();
    part.holes = vec![hole([50.0, 5.0], 6.0, 5.0, 1.0)];
    // Generic: 2 T = 4 and 2 T + R = 6 mm; the strictest, 6.
    let f = rule(&run("generic", &part), "hole.to_bend").into_iter().cloned().collect::<Vec<_>>();
    assert_eq!(f.len(), 1);
    assert_eq!((f[0].value, f[0].limit, f[0].severity), (Some(1.0), Some(6.0), Severity::Warning));
    assert!(f[0].message.contains("1 mm from bend 1 (to where the bend starts)") && f[0].message.contains("which differ"), "{}", f[0].message);
    // A hole inside a bend is told so.
    part.holes[0].bends[0].inside = true;
    part.holes[0].bends[0].from_start = 0.0;
    assert!(rule(&run("generic", &part), "hole.to_bend")[0].message.contains("is inside bend 1"));
}

#[test]
fn a_hole_near_an_edge_is_a_finding() {
    let mut part = bracket();
    part.holes = vec![hole([3.0, 30.0], 6.0, 3.0, 20.0)];
    // JLCCNC: 1.5 T = 3 mm, met; SendCutSend: 2 T = 4 mm, not met.
    assert!(rule(&run("jlccnc", &part), "hole.to_edge").is_empty());
    let f = rule(&run("sendcutsend", &part), "hole.to_edge").into_iter().cloned().collect::<Vec<_>>();
    assert_eq!((f.len(), f[0].limit, f[0].severity), (1, Some(4.0), Severity::Error));
}

#[test]
fn a_thickness_outside_the_stock_list_is_a_finding() {
    let mut part = bracket();
    part.thickness = 7.0;
    let r = run("jlccnc", &part);
    let f = rule(&r, "stock.thickness");
    assert_eq!(f.len(), 1);
    assert_eq!((f[0].value, f[0].severity), (Some(7.0), Severity::Error));
    assert!(f[0].message.contains("7 mm thick") && f[0].message.contains("its thickest stock is 6 mm"), "{}", f[0].message);
    part.thickness = 2.0;
    assert!(rule(&run("jlccnc", &part), "stock.thickness").is_empty());
    // PCBWay's pages disagree on the range: 0.5 to 6 mm is the strictest, so 7 mm is a warning.
    part.thickness = 7.0;
    assert_eq!(rule(&run("pcbway", &part), "stock.thickness")[0].severity, Severity::Warning);
}

#[test]
fn stock_is_looked_up_by_the_sheet_rules_material() {
    let mut part = bracket();
    // 0.080 in = 2.032 mm: 5052 stocks it, 304 stainless does not (0.074 and 0.100 in).
    part.thickness = 2.032;
    part.material = "Aluminium 5052-H32".into();
    assert!(rule(&run("sendcutsend", &part), "stock.thickness").is_empty());
    part.material = "Stainless steel 304".into();
    assert_eq!(rule(&run("sendcutsend", &part), "stock.thickness").len(), 1);
    // An unknown material tries every list, and says so.
    part.material = "Unobtainium".into();
    let r = run("sendcutsend", &part);
    assert!(rule(&r, "stock.thickness").is_empty());
    assert!(r.not_checked.iter().any(|n| n.rule == "stock.thickness" && n.why.contains("Unobtainium")));
}

#[test]
fn half_the_die_depends_on_the_material_and_is_measured_to_the_bend_line() {
    let mut part = bracket();
    part.holes = vec![hole([50.0, 5.0], 6.0, 5.0, 3.0)];
    // from_line = 3 + 2 = 5 mm. 5052 at 2 mm: half of a 12 mm die, 6 mm: a finding.
    part.material = "Aluminium 5052-H32".into();
    let f = rule(&run("sendcutsend", &part), "hole.to_bend").into_iter().cloned().collect::<Vec<_>>();
    assert_eq!((f.len(), f[0].limit, f[0].value), (1, Some(6.0), Some(5.0)));
    assert!(f[0].message.contains("(to the bend line)"));
    // Mild steel at 2 mm is in its 0.104 to 0.119 in step: a 16 mm die, 8 mm.
    part.material = "Mild steel (cold-rolled)".into();
    assert_eq!(rule(&run("sendcutsend", &part), "hole.to_bend")[0].limit, Some(8.0));
}

#[test]
fn a_flat_over_the_shops_largest_sheet_is_a_finding() {
    let mut part = bracket();
    part.flat_size = [300.0, 900.0];
    // JLCCNC: the longest side at most 800 mm.
    let f = rule(&run("jlccnc", &part), "size.max_flat").into_iter().cloned().collect::<Vec<_>>();
    assert_eq!((f.len(), f[0].value, f[0].limit), (1, Some(900.0), Some(800.0)));
    assert!(f[0].message.contains("900 x 300 mm") && f[0].message.contains("at most 800 mm long"), "{}", f[0].message);
    // PCBWay: 3000 by 1500 mm, in either way round.
    assert!(rule(&run("pcbway", &part), "size.max_flat").is_empty());
    part.flat_size = [1600.0, 3100.0];
    assert_eq!(rule(&run("pcbway", &part), "size.max_flat").len(), 1);
}

#[test]
fn threads_and_hardware_follow_what_the_shop_wants() {
    let mut part = bracket();
    part.threads =
        vec![ThreadFact { feature: 7, name: "Hole 3".into(), modelled: true }, ThreadFact { feature: 8, name: "Hole 4".into(), modelled: false }];
    part.hardware = vec![HardwareHole { feature: 9, label: "M3 nut".into(), at: [20.0, 20.0] }];
    let scs = run("sendcutsend", &part);
    let f = rule(&scs, "thread.modelling");
    assert_eq!((f.len(), f[0].feature, f[0].severity), (1, Some(7), Severity::Error), "only the modelled thread");
    let h = rule(&scs, "hardware.modelling");
    assert_eq!((h.len(), h[0].feature, h[0].severity), (1, Some(9), Severity::Warning));
    // JLCCNC gives no thread rule, and wants hardware modelled: nothing found, and both said.
    let jlc = run("jlccnc", &part);
    assert!(rule(&jlc, "thread.modelling").is_empty() && rule(&jlc, "hardware.modelling").is_empty());
    assert!(jlc.not_checked.iter().any(|n| n.rule == "thread.modelling"));
    assert!(jlc.not_checked.iter().any(|n| n.rule == "hardware.modelling" && n.why.contains("plain hole with a record")));
}

#[test]
fn one_body_one_thickness_and_what_was_not_measured() {
    let mut part = bracket();
    part.bodies = vec!["Body1".into(), "Body2".into()];
    part.recesses = 2;
    part.cutouts = 1;
    part.perforations = vec![(5, 0.4)];
    let r = run("jlccnc", &part);
    assert_eq!(rule(&r, "file.one_body").len(), 1);
    assert_eq!(rule(&r, "file.uniform_thickness").len(), 1);
    // The perforation's holes are held to the hole minimum like any hole: JLCCNC's strictest is 2 mm.
    assert!(rule(&r, "hole.min_diameter").iter().any(|f| f.feature == Some(5)));
    assert!(r.not_checked.iter().any(|n| n.rule == "hole.min_diameter" && n.why.contains("not round")));
}

#[test]
fn the_report_is_the_same_every_time() {
    let mut part = bracket();
    part.holes.push(hole([12.0, 8.0], 0.6, 3.0, 1.0));
    assert_eq!(run("pcbway", &part), run("pcbway", &part));
}

fn flange(number: usize, side: &'static str, from_start: f64) -> Flange {
    Flange { number, side, radius: 2.0, from_start, from_line: from_start + 2.0, at: [50.0, 10.0 * number as f64] }
}

/// A 20 mm flange passes a 4 T rule on 2 mm sheet (8 mm); a 6 mm one does not, and the finding says
/// 6 against 8, at the bend's middle.
#[test]
fn a_short_flange_is_a_finding_with_the_shops_limit() {
    let mut part = bracket();
    part.flanges = vec![flange(1, "left", 20.0), flange(1, "right", 30.0)];
    assert!(rule(&run("pcbway", &part), "flange.min").is_empty());
    part.flanges = vec![flange(1, "left", 6.0), flange(2, "left", 20.0)];
    // PCBWay's pages give 4 T and 2.5 T: the strictest, 8 mm, as a warning.
    let f = rule(&run("pcbway", &part), "flange.min").into_iter().cloned().collect::<Vec<_>>();
    assert_eq!(f.len(), 1);
    assert_eq!((f[0].value, f[0].limit, f[0].severity, f[0].at), (Some(6.0), Some(8.0), Severity::Warning, Some([50.0, 10.0])));
    assert!(
        f[0].message.starts_with("the flange on the left of bend 1 at (50, 10) reaches only 6 mm (from where the bend starts); PCBWay"),
        "{}",
        f[0].message
    );
    assert!(f[0].message.contains("at least 8 mm long") && f[0].message.contains("which differ"), "{}", f[0].message);
    // JLCCNC's one value, 3 T = 6 mm, is met by 6 mm, and is an error when not.
    assert!(rule(&run("jlccnc", &part), "flange.min").is_empty());
    part.flanges = vec![flange(1, "right", 5.0)];
    let f = rule(&run("jlccnc", &part), "flange.min").into_iter().cloned().collect::<Vec<_>>();
    assert_eq!((f.len(), f[0].limit, f[0].severity), (1, Some(6.0), Severity::Error));
}

/// A rule that says `bend_line` is held to the reach from the bend's centre line, and what could
/// not be measured is said.
#[test]
fn a_flange_rule_can_be_measured_from_the_bend_line() {
    let custom = Profile::from_value(&serde_json::json!({
        "name": "Acme",
        "rules": [{"id": "flange.min", "limits": [{
            "value": 4.0, "relative_to": "thickness", "from": "bend_line",
            "sources": [{"url": "https://example.com/rules", "read": "2026-11-25", "says": "four thicknesses from the bend line"}]
        }]}]
    }))
    .unwrap();
    let mut part = bracket();
    // 5 mm from where the bend starts is 7 from the line: under 8. 7 from the start is 9 from the
    // line, which passes.
    part.flanges = vec![flange(1, "left", 5.0), flange(1, "right", 7.0)];
    part.flange_gaps = vec!["1 flange(s) were not measured".into()];
    let r = check(&custom, &part);
    let f = rule(&r, "flange.min");
    assert_eq!((f.len(), f[0].value, f[0].limit), (1, Some(7.0), Some(8.0)));
    assert!(f[0].message.contains("(from the bend line)"), "{}", f[0].message);
    assert!(r.not_checked.iter().any(|n| n.rule == "flange.min" && n.why.contains("not measured")));
}

/// A part with two bends whose flat DXF the export would write with its defaults: the outline and
/// the cut-outs on two layers, the bend lines on theirs.
fn drawn() -> DxfFacts {
    DxfFacts {
        cut_layers: vec!["CUT".into(), "OUTLINE".into()],
        bends: 2,
        bend_lines: 2,
        bend_lines_with_cuts: false,
        slits: 0,
        unclosed: 0,
        text_notes: 0,
        single_layer: false,
        notes_asked: Vec::new(),
    }
}

fn filed(dxf: DxfFacts) -> Part {
    Part { dxf: Some(dxf), ..bracket() }
}

/// Two cut layers are a finding for a shop that wants one, with the option that fixes it; one
/// layer is not; a shop that states no file rule is told so.
#[test]
fn cut_contours_on_two_layers_are_a_finding_that_names_the_export_option() {
    let r = run("sendcutsend", &filed(drawn()));
    let f = rule(&r, "file.dxf_single_layer");
    assert_eq!(f.len(), 1);
    assert!(f[0].message.starts_with("the DXF would put the cut contours on 2 layers (CUT and OUTLINE); SendCutSend wants all cut geometry on one layer: export with layers: single"), "{}", f[0].message);
    assert!(f[0].message.contains("bend_lines: false"), "{}", f[0].message);
    assert_eq!(f[0].severity, Severity::Error);
    assert!(f[0].sources.iter().all(|u| u.starts_with("https://")) && !f[0].sources.is_empty());
    let mut one = drawn();
    one.cut_layers = vec!["0".into()];
    one.single_layer = true;
    one.bend_lines = 0;
    assert!(rule(&run("sendcutsend", &filed(one.clone())), "file.dxf_single_layer").is_empty());
    // Single layer with the bend lines still drawn: they are on the cut layer.
    one.bend_lines = 2;
    one.bend_lines_with_cuts = true;
    let f = rule(&run("pcbway", &filed(one)), "file.dxf_single_layer").into_iter().cloned().collect::<Vec<_>>();
    assert_eq!(f.len(), 1);
    assert!(
        f[0].message.contains("the 2 bend line(s) are on the cut layer too") && f[0].message.contains("export with bend_lines: false"),
        "{}",
        f[0].message
    );
    let jlc = run("jlccnc", &filed(drawn()));
    assert!(rule(&jlc, "file.dxf_single_layer").is_empty());
    assert!(jlc.not_checked.iter().any(|n| n.rule == "file.dxf_single_layer" && n.why.contains("JLCCNC's pages give no value")));
}

/// A junction cut is a line out and back; a contour the writer cannot close is another finding.
#[test]
fn contours_that_are_not_closed_are_findings() {
    assert!(rule(&run("generic", &filed(drawn())), "file.dxf_closed_contours").is_empty());
    let mut d = drawn();
    d.slits = 2;
    d.unclosed = 1;
    let f = rule(&run("generic", &filed(d)), "file.dxf_closed_contours").into_iter().cloned().collect::<Vec<_>>();
    assert_eq!(f.len(), 2);
    assert!(f[0].message.starts_with("2 cut(s) along the junction of a split edge are drawn in the DXF as a line out and back"), "{}", f[0].message);
    assert!(f[1].message.starts_with("1 cut contour(s) cannot be written to the DXF as closed shapes"), "{}", f[1].message);
}

/// SendCutSend wants a line at each bend in a flat file: leaving them out is a finding when the
/// part has bends, and not when it has none; on the cut layer they are not apart from the cuts. A
/// shop that wants none is held to the opposite.
#[test]
fn bend_lines_are_wanted_when_the_part_has_bends() {
    let mut d = drawn();
    assert!(rule(&run("sendcutsend", &filed(d.clone())), "file.dxf_bend_lines").is_empty());
    d.bend_lines = 0;
    let f = rule(&run("sendcutsend", &filed(d.clone())), "file.dxf_bend_lines").into_iter().cloned().collect::<Vec<_>>();
    assert_eq!(f.len(), 1);
    assert!(
        f[0].message.starts_with("the DXF leaves out the 2 bend line(s); SendCutSend wants a line at the centre of each bend")
            && f[0].message.ends_with("export with bend_lines: true"),
        "{}",
        f[0].message
    );
    d.bends = 0;
    assert!(rule(&run("sendcutsend", &filed(d)), "file.dxf_bend_lines").is_empty(), "a flat part needs none");
    let mut single = drawn();
    single.cut_layers = vec!["0".into()];
    single.single_layer = true;
    single.bend_lines_with_cuts = true;
    let f = rule(&run("sendcutsend", &filed(single.clone())), "file.dxf_bend_lines").into_iter().cloned().collect::<Vec<_>>();
    assert_eq!(f.len(), 1);
    assert!(f[0].message.contains("export with layers: roles"), "{}", f[0].message);
    // A shop that would cut the lines.
    let none = Profile::from_value(&serde_json::json!({
        "name": "Acme",
        "rules": [{"id": "file.dxf_bend_lines", "bend_lines": "absent",
            "sources": [{"url": "https://example.com/rules", "read": "2026-11-25", "says": "no bend lines in the file"}]}]
    }))
    .unwrap();
    let f = rule(&check(&none, &filed(drawn())), "file.dxf_bend_lines").into_iter().cloned().collect::<Vec<_>>();
    assert_eq!(f.len(), 1);
    assert!(f[0].message.contains("draws 2 bend line(s); Acme would cut them: export with bend_lines: false"), "{}", f[0].message);
    let mut off = drawn();
    off.bend_lines = 0;
    assert!(rule(&check(&none, &filed(off)), "file.dxf_bend_lines").is_empty());
}

/// Notes are text entities: asking for them is a finding for a shop that wants outlines.
#[test]
fn notes_written_as_text_are_a_finding() {
    assert!(rule(&run("pcbway", &filed(drawn())), "file.text_outlined").is_empty());
    let mut d = drawn();
    d.text_notes = 3;
    d.notes_asked = vec!["bend_notes", "hardware_notes"];
    let f = rule(&run("pcbway", &filed(d)), "file.text_outlined").into_iter().cloned().collect::<Vec<_>>();
    assert_eq!(f.len(), 1);
    assert!(
        f[0].message.contains("3 note(s) as text entities") && f[0].message.contains("export without bend_notes and hardware_notes"),
        "{}",
        f[0].message
    );
}

/// With no DXF read, the file rules are listed as not checked, never passed.
#[test]
fn the_file_rules_are_not_checked_without_the_dxf() {
    let r = run("sendcutsend", &bracket());
    for id in ["file.dxf_single_layer", "file.dxf_closed_contours", "file.dxf_bend_lines", "file.text_outlined"] {
        assert!(!r.checked.contains(&id.to_string()), "{id}");
        assert!(r.not_checked.iter().any(|n| n.rule == id && n.why.contains("not read")), "{id}");
    }
}
