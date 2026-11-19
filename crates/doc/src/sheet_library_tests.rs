use super::*;
use crate::Document;
use crate::sheet::RuleValues;

fn values(rule: &SheetRule) -> RuleValues {
    match Document::default().rule_values(&std::collections::BTreeMap::new(), rule) {
        Ok(v) => v,
        Err(e) => panic!("{}: {e}", rule.name),
    }
}

/// The thickness a library row gives, in inches, as the table prints it.
fn inches(r: &PackRule) -> f64 {
    let text = r.rule.thickness.strip_suffix(" in").unwrap_or_else(|| panic!("{} is not in inches", r.rule.name));
    text.parse().unwrap_or_else(|_| panic!("{}", r.rule.thickness))
}

fn gauge_number(r: &PackRule) -> i32 {
    r.gauge.strip_suffix(" ga").and_then(|g| g.parse().ok()).unwrap_or_else(|| panic!("{}: gauge `{}`", r.rule.name, r.gauge))
}

fn rounded4(x: f64) -> f64 {
    (x * 1e4).round() / 1e4
}

#[test]
fn the_standard_library_has_the_issues_materials_each_value_with_its_sources() {
    let lib = standard();
    assert_eq!(
        standard_materials(),
        ["Mild steel (cold-rolled)", "Stainless steel 304", "Aluminium 5052-H32", "Aluminium 6061-T6", "Copper C110", "Brass C260"]
    );
    assert_eq!(lib.rules.len(), 55);
    for source in &lib.sources {
        assert!(source.url.starts_with("https://") && !source.title.is_empty(), "{source:?}");
    }
    for r in &lib.rules {
        // A gauge source, the K-factor chart, and a radius source.
        assert_eq!(r.sources.len(), 3, "{}", r.rule.name);
        assert!(r.sources.contains(&"kfactor".to_string()), "{}", r.rule.name);
        assert_eq!(r.rule.name, format!("{}, {}", r.rule.material, r.gauge));
        values(&r.rule);
    }
}

/// Manufacturers' Standard Gauge: the weight of a square foot of the gauge (lb), over 41.82 lb
/// per square foot per inch of thickness, rounded to 0.0001 in as gauge tables print it.
#[test]
fn steel_gauges_are_the_manufacturers_standard_gauge() {
    let weights = [(10, 5.625), (11, 5.0), (12, 4.375), (13, 3.75), (14, 3.125), (16, 2.5), (18, 2.0), (20, 1.5), (22, 1.25), (24, 1.0)];
    let rows = standard_gauges("Mild steel (cold-rolled)");
    assert_eq!(rows.len(), weights.len());
    for (gauge, lb) in weights {
        let r = rows.iter().find(|r| gauge_number(r) == gauge).unwrap_or_else(|| panic!("no {gauge} ga"));
        assert_eq!(inches(r), rounded4(lb / 41.82), "{}", r.rule.name);
    }
    // The values every gauge table prints.
    assert_eq!(standard_rule("mild steel (cold-rolled)", "16").map(inches), Some(0.0598));
    assert_eq!(standard_rule("Mild steel (cold-rolled)", "10 gauge").map(inches), Some(0.1345));
}

/// 15 U.S.C. 206: each gauge is a fraction of an inch.
#[test]
fn stainless_gauges_are_the_us_standard_gauge_of_15_usc_206() {
    let fractions = [
        (10, 9.0 / 64.0),
        (11, 1.0 / 8.0),
        (12, 7.0 / 64.0),
        (14, 5.0 / 64.0),
        (16, 1.0 / 16.0),
        (18, 1.0 / 20.0),
        (20, 3.0 / 80.0),
        (22, 1.0 / 32.0),
        (24, 1.0 / 40.0),
    ];
    let rows = standard_gauges("Stainless steel 304");
    assert_eq!(rows.len(), fractions.len());
    for (gauge, inch) in fractions {
        let r = rows.iter().find(|r| gauge_number(r) == gauge).unwrap_or_else(|| panic!("no {gauge} ga"));
        assert_eq!(inches(r), inch, "{}", r.rule.name);
    }
}

/// Brown & Sharpe (ASTM B258): d = 0.005 in × 92^((36 − n) / 39), rounded to 0.0001 in.
#[test]
fn aluminium_copper_and_brass_gauges_are_brown_and_sharpe() {
    for material in ["Aluminium 5052-H32", "Aluminium 6061-T6", "Copper C110", "Brass C260"] {
        let rows = standard_gauges(material);
        assert!(rows.len() >= 8, "{material}");
        for r in rows {
            let n = gauge_number(r);
            assert_eq!(inches(r), rounded4(0.005 * 92f64.powf(f64::from(36 - n) / 39.0)), "{}", r.rule.name);
        }
    }
    // The values the tables print: 16 ga is 0.0508 in, 10 ga 0.1019 in.
    assert_eq!(standard_rule("Aluminium 5052-H32", "16 ga").map(inches), Some(0.0508));
    assert_eq!(standard_rule("Copper C110", "10ga").map(inches), Some(0.1019));
}

/// The generic air-bending chart: by inside radius over thickness (0 to T, T to 3T, over 3T)
/// and by soft (aluminium; copper and brass with it), medium (steel) and hard (stainless).
#[test]
fn k_factors_follow_the_generic_air_bending_chart() {
    let chart = |hardness: usize, r_over_t: f64| -> f64 {
        let band = if r_over_t < 1.0 {
            [0.33, 0.38, 0.40]
        } else if r_over_t <= 3.0 {
            [0.40, 0.43, 0.45]
        } else {
            [0.50, 0.50, 0.50]
        };
        band[hardness]
    };
    for r in &standard().rules {
        let hardness = match r.rule.material.as_str() {
            "Mild steel (cold-rolled)" => 1,
            "Stainless steel 304" => 2,
            _ => 0,
        };
        let v = values(&r.rule);
        assert!((v.k_factor - chart(hardness, v.bend_radius / v.thickness)).abs() < 1e-12, "{}: K {}", r.rule.name, v.k_factor);
    }
}

/// 6061-T6's approximate minimum radius for a 90° cold bend, the top of each range, by the
/// thickness the chart lists at or above the sheet's: 1/64 in 1T, 1/32 in 1-1/2T, 1/16 in 2T,
/// 1/8 in 3T, 3/16 in 4T. The other materials bend at one thickness.
#[test]
fn bend_radii_follow_the_published_minimum_radii() {
    let bands = [(1.0 / 64.0, 1.0), (1.0 / 32.0, 1.5), (1.0 / 16.0, 2.0), (1.0 / 8.0, 3.0), (3.0 / 16.0, 4.0)];
    for r in &standard().rules {
        let v = values(&r.rule);
        let want = if r.rule.material == "Aluminium 6061-T6" {
            bands.iter().find(|(t, _)| inches(r) <= *t).map(|(_, m)| *m).unwrap_or_else(|| panic!("{}", r.rule.name))
        } else {
            1.0
        };
        assert!((v.bend_radius / v.thickness - want).abs() < 1e-12, "{}: {} T", r.rule.name, v.bend_radius / v.thickness);
    }
}

#[test]
fn a_pack_round_trips_through_json_and_csv() {
    let lib = standard().clone();
    let json = write_json(&lib);
    let back = read(json.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back.pack, lib);
    assert!(back.skipped.is_empty(), "{:?}", back.skipped);
    assert_eq!(write_json(&back.pack), json);
    // The shipped file is this format as written.
    let shipped = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sheet-metal/standard-library.json"));
    assert_eq!(read(shipped.as_bytes()).map(|r| r.pack).as_ref(), Ok(&lib));
    // CSV keeps the rules (not the sources), and a formula-like text survives it.
    let mut odd = lib.rules[0].clone();
    odd.rule.name = "=1+1, \"quoted\"".into();
    odd.rule.material = "-steel".into();
    let pack = RulePack { rules: vec![odd.clone(), lib.rules[20].clone()], ..RulePack::default() };
    let csv = write_csv(&pack);
    let back = read(csv.as_bytes()).unwrap_or_else(|e| panic!("{e}\n{csv}"));
    let strip = |mut r: PackRule| {
        r.sources.clear();
        r
    };
    assert_eq!(back.pack.rules, pack.rules.into_iter().map(strip).collect::<Vec<_>>());
    assert!(csv.lines().nth(1).is_some_and(|l| l.starts_with("\"'=1+1")), "{csv}");
}

#[test]
fn a_short_csv_pack_takes_the_default_rule_for_what_it_leaves_out() {
    let csv = "\u{feff}Name,Thickness,K_Factor,Bend_Radius,Material\r\nShop CRS 16,1.52 mm,0.42,\"0.8 * Thickness\",CRS\r\n\r\nShop AL 3,3 mm,0.45,3 mm,\r\n";
    let read = read(csv.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    let first = &read.pack.rules[0].rule;
    assert_eq!((first.name.as_str(), first.material.as_str(), first.bend_radius.as_str()), ("Shop CRS 16", "CRS", "0.8 * Thickness"));
    assert_eq!(first.gap, SheetRule::steel().gap);
    assert_eq!(read.pack.rules.len(), 2);
}

#[test]
fn a_newer_pack_is_read_for_what_this_version_knows_and_names_the_rest() {
    let text = r#"{"format":"solidcraft-sheet-rules","version":3,"shop":"x","rules":[
        {"name":"A","thickness":"2 mm","k_factor":0.4,"bend_radius":"2 mm","min_flange":"8 mm","tolerance":{"plus":"0.1 mm"}}]}"#;
    let read = read(text.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read.version, 3);
    assert_eq!(read.skipped, ["min_flange", "shop", "tolerance"]);
    assert_eq!(read.pack.rules[0].rule.k_factor, "0.4");
}

fn refused(text: &str) -> String {
    match read(text.as_bytes()) {
        Ok(r) => panic!("read {text}: {r:?}"),
        Err(e) => e.to_string(),
    }
}

fn row(fields: &str) -> String {
    format!(r#"{{"format":"solidcraft-sheet-rules","version":1,"rules":[{{"name":"R",{fields}}}]}}"#)
}

#[test]
fn hostile_packs_are_refused_with_what_to_change() {
    let ok = r#""thickness":"2 mm","k_factor":"0.4","bend_radius":"2 mm""#;
    assert!(read(row(ok).as_bytes()).is_ok());
    // A material label is counted in characters: 128 of `é` is 256 bytes and fine.
    assert!(read(row(&format!(r#""material":"{}",{ok}"#, "é".repeat(128))).as_bytes()).is_ok());
    assert!(refused(&row(&format!(r#""material":"{}",{ok}"#, "é".repeat(129)))).contains("128 characters"));
    let cases: Vec<(String, &str)> = vec![
        ("x".repeat(MAX_PACK_BYTES + 1), "at most"),
        (format!("{}{}", "[".repeat(5000), "]".repeat(5000)), "not valid JSON"),
        ("{\"format\":\"solidcraft-sheet-rules\"".into(), "not valid JSON"),
        (r#"{"rules":[]}"#.into(), "not a sheet metal rule pack"),
        (r#"[{"name":"A"}]"#.into(), "not a sheet metal rule pack"),
        (r#"{"format":"solidcraft-sheet-rules","version":0,"rules":[]}"#.into(), "whole number"),
        (r#"{"format":"solidcraft-sheet-rules","version":-1,"rules":[]}"#.into(), "whole number"),
        (r#"{"format":"solidcraft-sheet-rules","rules":{}}"#.into(), "must be a list"),
        (r#"{"format":"solidcraft-sheet-rules","rules":[]}"#.into(), "no rules"),
        (r#"{"format":"solidcraft-sheet-rules","rules":[7]}"#.into(), "an object"),
        (row(r#""thickness":"2 mm""#), "`k_factor` is required"),
        (row(r#""thickness":"2","k_factor":"0.4","bend_radius":"2 mm""#), "no unit"),
        (row(r#""thickness":"T = 2 mm","k_factor":"0.4","bend_radius":"2 mm""#), "not a parameter definition"),
        (row(r#""thickness":"Thick","k_factor":"0.4","bend_radius":"2 mm""#), "only the rule's own `Thickness`"),
        (row(r#""thickness":"Thickness","k_factor":"0.4","bend_radius":"2 mm""#), "only the rule's own `Thickness`"),
        (row(r#""thickness":"2 mm","k_factor":"1.5","bend_radius":"2 mm""#), "K-factor must be between 0 and 1"),
        (row(r#""thickness":"2 mm","k_factor":"0.4","bend_radius":"0 mm""#), "more than 0"),
        (row(r#""thickness":"-2 mm","k_factor":"0.4","bend_radius":"2 mm""#), "thickness must be more than 0"),
        (row(r#""thickness":"2 deg","k_factor":"0.4","bend_radius":"2 mm""#), "rule 1 of the pack (`R`)"),
        (row(r#""thickness":"2 mm","k_factor":"0.4","bend_radius":"2 mm","corner_relief_shape":"star""#), "notch, round"),
        (row(r#""thickness":"2 mm","k_factor":"0.4","bend_radius":"2 mm","angle_display":"x""#), "bend or included"),
        (row(r#""thickness":"2 mm","k_factor":{"a":1},"bend_radius":"2 mm""#), "text or a number"),
        (row(&format!(r#""thickness":"2 mm","k_factor":"0.4","bend_radius":"{}""#, "1".repeat(600))), "at most 512"),
        (row(r#""thickness":"2 mm\u0007","k_factor":"0.4","bend_radius":"2 mm""#), "one line"),
        (row(r#""material":"a\nb","thickness":"2 mm","k_factor":"0.4","bend_radius":"2 mm""#), "one line"),
        (row(r#""thickness":"1 + 1","k_factor":"0.4","bend_radius":"2 mm""#), "no unit"),
        (row(r#""thickness":"2 mm","k_factor":"0.4","bend_radius":"(2)""#), "no unit"),
        (row(r#""thickness":"2 mm","k_factor":"0.4","bend_radius":"2 * Thickness / Thickness""#), "no unit"),
        ("name,thickness,bend_radius,k_factor\nR,2 mm,2 mm,0,44\n".into(), "5 values for 4 columns"),
        (row(r#""thickness":"2 mm","k_factor":"0.4","bend_radius":"2 mm","source":"nowhere""#), "not in the pack's sources"),
        (format!(r#"{{"format":"solidcraft-sheet-rules","rules":[{{"name":"A",{ok}}},{{"name":"a",{ok}}}]}}"#), "two rules named"),
        ("name,thickness,k_factor\nA,2 mm,0.4\n".into(), "`bend_radius` column"),
        ("name,thickness,k_factor,bend_radius\n\"A,2 mm,0.4,2 mm\n".into(), "unmatched quotes"),
        ("name,thickness,k_factor,bend_radius\n".into(), "no rules"),
        ("".into(), "no rules"),
    ];
    for (text, want) in cases {
        let why = refused(&text);
        assert!(why.contains(want), "{}: {why}", &text[..text.len().min(120)]);
    }
    let many = format!("name,thickness,k_factor,bend_radius\n{}", (0..=MAX_RULES).map(|i| format!("R{i},2 mm,0.4,2 mm\n")).collect::<String>());
    assert!(refused(&many).contains("more than 1000"));
    assert!(read(&[0xff, 0xfe, 0x00]).is_err_and(|e| e == PackError::NotText));
}
