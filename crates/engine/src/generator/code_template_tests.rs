//! Field templates: reading a code, printing parameters back, and hostile input.

use std::collections::BTreeMap;

use serde_json::{Value as Json, json};

use super::code_template::{Decl, Declared, FieldKind, MATCH_BUDGET, Params, Template, TemplateError, match_any};

fn declared() -> Declared {
    let mut d = Declared::new();
    d.insert("size".into(), Decl { kind: FieldKind::Choice(["7", "9", "12", "15"].map(String::from).to_vec()), default: None });
    d.insert("blocks".into(), Decl { kind: FieldKind::Int { min: 1, max: 4, step: 1 }, default: Some(json!(1)) });
    d
}

const MGN: &str = "MGN{size}{block: C=standard, H=heavy}[E:long]{blocks}R{length: int 40..2000}";

fn params(v: Json) -> Params {
    v.as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<BTreeMap<_, _>>()).unwrap_or_default()
}

#[test]
fn a_code_reads_into_typed_parameters_and_prints_back() {
    let t = Template::parse(MGN, &declared()).unwrap();
    let p = t.match_code("MGN15C2R600").unwrap().unwrap();
    assert_eq!(p, params(json!({"size": "15", "block": "standard", "long": false, "blocks": 2, "length": 600})));
    assert_eq!(t.print(&p).unwrap(), "MGN15C2R600");
}

#[test]
fn optional_flag_and_letter_maps_round_trip() {
    let t = Template::parse(MGN, &declared()).unwrap();
    for code in ["MGN15CE1R40", "MGN7HE4R2000", "MGN12C3R1999"] {
        let p = t.match_code(code).unwrap().unwrap_or_default();
        assert!(!p.is_empty(), "{code}");
        assert_eq!(t.print(&p).unwrap(), code);
        assert_eq!(t.match_code(&t.print(&p).unwrap()).unwrap(), Some(p));
    }
    let p = t.match_code("mgn-15-h-e-2-r-600").unwrap().unwrap();
    assert_eq!(p["block"], "heavy");
    assert_eq!(p["long"], true);
    assert_eq!(t.print(&p).unwrap(), "MGN15HE2R600");
}

#[test]
fn the_longest_choice_wins_and_a_later_failure_backs_up() {
    let mut d = Declared::new();
    d.insert("a".into(), Decl { kind: FieldKind::Choice(vec!["1".into(), "12".into()]), default: None });
    let t = Template::parse("{a}{b: int 1..9}", &d).unwrap();
    // "12" would leave nothing for b, so a is "1" and b is 2.
    assert_eq!(t.match_code("12").unwrap().unwrap(), params(json!({"a": "1", "b": 2})));
    assert_eq!(t.match_code("123").unwrap().unwrap(), params(json!({"a": "12", "b": 3})));
}

#[test]
fn numbers_are_checked_for_range_step_and_leading_zeros() {
    let t = Template::parse("L{n: int 20..100 step 5}", &Declared::new()).unwrap();
    assert!(t.match_code("L25").unwrap().is_some());
    assert!(t.match_code("L26").unwrap().is_none());
    assert!(t.match_code("L15").unwrap().is_none());
    assert!(t.match_code("L0025").unwrap().is_none());
    assert!(matches!(t.print(&params(json!({"n": 26}))), Err(TemplateError::Field { .. })));
    let d = Template::parse("W{w: decimal 0.5..9 step 0.5 digits 1}", &Declared::new()).unwrap();
    assert_eq!(d.match_code("W2.5").unwrap().unwrap()["w"], json!(2.5));
    assert!(d.match_code("W2.7").unwrap().is_none());
    assert!(d.match_code("W2.55").unwrap().is_none());
    assert_eq!(d.print(&params(json!({"w": 3.0}))).unwrap(), "W3");
}

#[test]
fn an_optional_part_gives_its_declared_default_when_left_out() {
    let t = Template::parse("X{size}[-{blocks}]", &declared()).unwrap();
    assert_eq!(t.match_code("X9").unwrap().unwrap(), params(json!({"size": "9", "blocks": 1})));
    assert_eq!(t.match_code("X9-3").unwrap().unwrap()["blocks"], json!(3));
    assert_eq!(t.print(&params(json!({"size": "9", "blocks": 1}))).unwrap(), "X9");
    assert_eq!(t.print(&params(json!({"size": "9", "blocks": 3}))).unwrap(), "X9-3");
}

#[test]
fn several_templates_are_tried_in_order() {
    let a = Template::parse("A{n: int 1..9}", &Declared::new()).unwrap();
    let b = Template::parse("B{n: int 1..9}", &Declared::new()).unwrap();
    let (i, p) = match_any(&[a.clone(), b.clone()], "b7").unwrap().unwrap();
    assert_eq!((i, p["n"].clone()), (1, json!(7)));
    assert!(match_any(&[a, b], "C7").unwrap().is_none());
}

#[test]
fn a_text_field_takes_its_escape_characters() {
    let mut d = Declared::new();
    let extra = super::code_template::CharSet::parse("[A-Za-z0-9/]").unwrap();
    d.insert("id".into(), Decl { kind: FieldKind::Text { min: 1, max: 8, extra }, default: None });
    let t = Template::parse("P{id}", &d).unwrap();
    assert_eq!(t.match_code("PAB/12").unwrap().unwrap()["id"], json!("AB/12"));
    assert!(super::code_template::CharSet::parse("A-Z").is_err());
}

#[test]
fn a_malformed_template_is_refused_in_words() {
    for src in ["", "A{", "A}", "A[", "A]", "A[]", "{x: int 5..1}", "{x: nope 1..2}", "{1x: int 1..2}", "{size}", "{x: int 1..2}{x: text 0..3}", "[[[[[[A]]]]]]", "{a: A=}"] {
        assert!(Template::parse(src, &Declared::new()).is_err(), "{src:?}");
    }
}

#[test]
fn hostile_input_is_refused_not_run_away_with() {
    let t = Template::parse(MGN, &declared()).unwrap();
    assert!(matches!(t.match_code(&"9".repeat(129)), Err(TemplateError::Limit { .. })));
    assert!(matches!(Template::parse(&"A".repeat(257), &Declared::new()), Err(TemplateError::Limit { .. })));
    let many = (0..40).map(|i| format!("{{f{i}: int 0..9}}")).collect::<String>();
    assert!(matches!(Template::parse(&many, &Declared::new()), Err(TemplateError::Limit { .. })));
    for code in ["", " ", "\u{0}", "MGN\u{202e}15", "ＭＧＮ15", "MGN15C2R", "MGN15C2R99999999999999999999", "-----", "🦀🦀🦀"] {
        let _ = t.match_code(code);
    }
    // Many ways to split the same digits: the step budget ends the search.
    let src = (0..10).map(|i| format!("{{n{i}: int 0..99999}}")).collect::<String>();
    let greedy = Template::parse(&src, &Declared::new()).unwrap();
    let r = greedy.match_code(&format!("{}x", "1".repeat(100)));
    assert!(matches!(r, Ok(None) | Err(TemplateError::Limit { .. })));
    assert!(MATCH_BUDGET > 0);
}

#[test]
fn an_option_set_reads_in_any_order_and_prints_in_the_declared_one() {
    let t = Template::parse("HFS5-{w: int 20..100}{o: set(-LTP, -RTP, -SC:int 0..99)}", &Declared::new()).unwrap();
    let a = t.match_code("HFS5-30-RTP-LTP-SC12").unwrap().unwrap();
    let b = t.match_code("hfs5 30 sc12 ltp rtp").unwrap().unwrap();
    assert_eq!(a, b);
    assert_eq!(a["o"], json!({"LTP": true, "RTP": true, "SC": 12}));
    assert_eq!(t.print(&a).unwrap(), "HFS5-30-LTP-RTP-SC12");
    assert_eq!(t.match_code("HFS5-30").unwrap().unwrap()["o"], json!({}));
    assert!(t.match_code("HFS5-30-LTP-LTP").unwrap().is_none());
    assert!(matches!(t.print(&params(json!({"w": 30, "o": {"NOPE": true}}))), Err(TemplateError::Field { .. })));
}

#[test]
fn a_free_tail_keeps_what_is_typed() {
    let t = Template::parse("MGN{n: int 1..9}[+{tail: rest}]", &Declared::new()).unwrap();
    assert_eq!(t.match_code("MGN5+SPECIAL-X").unwrap().unwrap()["tail"], json!("SPECIAL-X"));
    assert!(t.match_code("MGN5").unwrap().is_some());
    assert_eq!(t.print(&params(json!({"n": 5, "tail": "A-1"}))).unwrap(), "MGN5+A-1");
}

#[test]
fn a_required_separator_must_be_there() {
    let t = Template::parse(r"E-SFJ{d: int 1..99}\-{l: int 1..99}", &Declared::new()).unwrap();
    assert_eq!(t.match_code("e sfj 12-5").unwrap().unwrap(), params(json!({"d": 12, "l": 5})));
    assert!(t.match_code("E-SFJ125").unwrap().is_none());
    assert_eq!(t.print(&params(json!({"d": 12, "l": 5}))).unwrap(), "E-SFJ12-5");
}

#[test]
fn a_template_names_its_literal_prefixes_for_an_index() {
    let t = Template::parse(MGN, &declared()).unwrap();
    assert_eq!(t.prefixes(), ["MGN7", "MGN9", "MGN12", "MGN15"].map(String::from).to_vec());
}
