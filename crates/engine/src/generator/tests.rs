//! Generator package tests: the example package, the expression language and hostile packages.

use serde_json::{Value as Json, json};

use super::expr::{self, Budget, EvalError, Scope, Value};
use super::{Generator, GeneratorError, RefusalKind};

const EXAMPLE: &str = include_str!("../../../../examples/generators/slot-nut-20.generator.json");
const NOW: &str = "0.51.0";

fn example() -> Generator {
    Generator::load(EXAMPLE, NOW).unwrap()
}

fn ev(src: &str) -> Result<Value, EvalError> {
    let e = expr::parse(src).map_err(EvalError::Msg)?;
    expr::eval(&e, &Scope::new(), &mut Budget { remaining: 100_000 })
}

fn num(src: &str) -> f64 {
    match ev(src) {
        Ok(Value::Num(n)) => n,
        other => panic!("`{src}` gave {other:?}"),
    }
}

fn fails(src: &str) -> String {
    match ev(src) {
        Err(EvalError::Msg(m)) => m,
        other => panic!("`{src}` should fail, gave {other:?}"),
    }
}

/// A package with the given steps, one table of `rows` rows and a code `X<n>`.
fn package(steps: Json) -> String {
    json!({
        "format": "solidcraft-generator/1",
        "id": "hostile-test",
        "name": "Hostile test",
        "grammar": {"alternatives": [[{"lit": "X"}, {"field": "n", "int": {"min": 0, "max": 100000}}]]},
        "steps": steps
    })
    .to_string()
}

fn load(text: &str) -> Result<Generator, GeneratorError> {
    Generator::load(text, NOW)
}

#[test]
fn the_example_package_loads_and_its_declarations_hold() {
    let g = example();
    assert_eq!(g.meta.id, "example-slot-nut-20");
    assert_eq!(g.meta.guarantee.as_deref(), Some("checked"));
    for (code, positive) in &g.declared {
        assert_eq!(g.parse(code).is_ok(), *positive, "declared code {code}");
    }
}

#[test]
fn a_code_becomes_fields_rows_and_commands() {
    let g = example();
    let plan = g.plan("TN20-M4").unwrap();
    assert_eq!(plan.component.as_deref(), Some("TN20-M4"));
    assert_eq!(plan.parsed.rows.len(), 1);
    assert_eq!(plan.parsed.rows[0].status.as_deref(), Some("approximate"));
    assert_eq!(plan.parsed.rows[0].source.as_deref(), Some("S1"));
    assert_eq!(plan.commands.len(), 2);
    assert_eq!(plan.commands[0].command, "solid.box");
    assert_eq!(plan.commands[0].params["length"], json!(10));
    assert_eq!(plan.commands[0].params["name"], json!("TN20-M4 body"));
    assert_eq!(plan.commands[1].command, "solid.cylinder");
    assert_eq!(plan.commands[1].params["diameter"], json!(3.3));
    assert_eq!(plan.commands[1].params["base"], json!([5, 5, -1]));
    assert_eq!(plan.commands[1].params["name"], json!("Tapped hole 1"));
}

#[test]
fn an_optional_suffix_repeats_a_step_and_case_is_ignored() {
    let g = example();
    let plan = g.plan("tn20-m5-2").unwrap();
    assert_eq!(plan.commands.len(), 3);
    assert_eq!(plan.commands[1].params["base"], json!([4, 5.5, -1]));
    assert_eq!(plan.commands[2].params["base"], json!([8, 5.5, -1]));
    assert_eq!(Generator::named_value(&plan.parsed, "holes"), Some(2.0));
}

#[test]
fn refusals_carry_the_packages_words_and_kind() {
    let g = example();
    let kind_and_text = |code: &str| match g.parse(code) {
        Err(GeneratorError::Refused { kind, message }) => (kind, message),
        other => panic!("{code}: {other:?}"),
    };
    assert_eq!(kind_and_text("TN20-M3-2"), (RefusalKind::Invalid, "The two-hole nut is not offered in M3.".into()));
    assert_eq!(kind_and_text("TN20-M6-2").0, RefusalKind::NotModelled);
    assert!(kind_and_text("TN20-M7").1.starts_with("TN20-M7 is not a TN20 ordering code"));
    assert!(kind_and_text("TN20-M4-3").1.contains("not a TN20"));
    assert!(kind_and_text("").1.contains("not a TN20"));
}

#[test]
fn hostile_codes_are_refused_never_panic() {
    let g = example();
    let long = "TN20-M4".to_string() + &"9".repeat(10_000);
    assert!(matches!(g.parse(&long), Err(GeneratorError::Limit { .. })));
    let m128 = "M".repeat(128);
    for code in
        ["TN20-M4\0", "\u{202e}TN20-M4", "ＴＮ20-M4", "TN20-M4-\u{1F600}", "TN20-", "-", "TN20-M4--2", "TN20-M4-02", "TN20-M4-2-2", m128.as_str()]
    {
        assert!(g.parse(code).is_err(), "{code:?}");
    }
    assert!(g.parse("  TN20-M4  ").is_ok());
}

#[test]
fn arithmetic_functions_and_text() {
    assert_eq!(num("1 + 2 * 3"), 7.0);
    assert_eq!(num("(1 + 2) * 3"), 9.0);
    assert_eq!(num("-2 * -3"), 6.0);
    assert_eq!(num("7 % 4"), 3.0);
    assert_eq!(num("min(3, 1, 2) + max(1, 5)"), 6.0);
    assert_eq!(num("round(2.4567, 2)"), 2.46);
    assert!((num("tan(45)") - 1.0).abs() < 1e-12);
    assert!((num("sin(30)") - 0.5).abs() < 1e-12);
    assert!((num("atan2(1, 1)") - 45.0).abs() < 1e-12);
    assert_eq!(num("true ? 1 : 2"), 1.0);
    assert_eq!(num("if(1 > 2, 10, 20)"), 20.0);
    assert_eq!(ev("\"M\" + 4").unwrap(), Value::Text("M4".into()));
    assert_eq!(ev("concat(\"M\", 2 + 2, \"x\", fmt(1.5, 2))").unwrap(), Value::Text("M4x1.50".into()));
    assert_eq!(ev("upper(\"ab\")").unwrap(), Value::Text("AB".into()));
    assert_eq!(ev("1 == 1 && \"a\" != \"b\"").unwrap(), Value::Bool(true));
    // Short-circuit: the right side is never evaluated.
    assert_eq!(ev("false && (1 / 0 > 1)").unwrap(), Value::Bool(false));
    assert_eq!(num("if(true, 1, 1 / 0)"), 1.0);
}

#[test]
fn expression_errors_are_typed_never_panics_or_nan() {
    assert!(fails("1 / 0").contains("division by zero"));
    assert!(fails("5 % 0").contains("zero"));
    assert!(fails("0 / 0").contains("zero"));
    assert!(fails("1e308 * 10").contains("not a finite"));
    assert!(fails("pow(10, 400)").contains("not a finite"));
    assert!(fails("sqrt(-1)").contains("negative"));
    assert!(fails("tan(90)").contains("tan"));
    assert!(fails("asin(2)").contains("-1 to 1"));
    assert!(fails("nope + 1").contains("unknown name"));
    assert!(fails("s.x").contains("unknown name"));
    assert!(fails("1 + true").contains("needs a number"));
    assert!(fails("!1").contains("true or false"));
    assert!(fails("frobnicate(1)").contains("unknown function"));
    assert!(fails("min()").contains("argument"));
    assert!(fails("fmt(1, 99)").contains("0 to 12"));
    assert!(fails("1 +").contains("too soon"));
    assert!(fails("1 2").contains("follows"));
    assert!(fails("\"open").contains("not closed"));
    assert!(fails("1e999").contains("finite"));
    assert!(fails("#").contains("unexpected"));
}

#[test]
fn deep_long_and_busy_expressions_hit_their_limits() {
    let deep = "(".repeat(40) + "1" + &")".repeat(40);
    assert!(expr::parse(&deep).unwrap_err().contains("nested"));
    let unary = "-".repeat(40) + "1";
    assert!(expr::parse(&unary).unwrap_err().contains("nested"));
    assert!(expr::parse(&("1+".repeat(300) + "1")).is_err());
    assert!(expr::parse(&"1".repeat(600)).unwrap_err().contains("longer"));
    let text = format!("\"{}\"", "a".repeat(5000));
    assert!(expr::parse(&text).is_err());
    let e = expr::parse("1 + 2 + 3 + 4").unwrap();
    assert_eq!(expr::eval(&e, &Scope::new(), &mut Budget { remaining: 3 }), Err(EvalError::Budget));
}

#[test]
fn a_package_that_is_not_one_is_refused() {
    assert!(matches!(load("not json"), Err(GeneratorError::Package(_))));
    assert!(matches!(load("[]"), Err(GeneratorError::Package(_))));
    let mut v: Json = serde_json::from_str(EXAMPLE).unwrap();
    v["surprise"] = json!(1);
    assert!(matches!(load(&v.to_string()), Err(GeneratorError::Package(m)) if m.contains("surprise")));
    v.as_object_mut().unwrap().remove("surprise");
    v["x-note"] = json!("kept");
    assert!(load(&v.to_string()).is_ok());
    v["format"] = json!("solidcraft-generator/2");
    assert!(matches!(load(&v.to_string()), Err(GeneratorError::NeedsNewer { .. })));
    v["format"] = json!("solidcraft-generator/1");
    v["min_solidcraft"] = json!("9.0.0");
    assert!(matches!(load(&v.to_string()), Err(GeneratorError::NeedsNewer { .. })));
    v["min_solidcraft"] = json!("soon");
    assert!(load(&v.to_string()).is_err());
    v["min_solidcraft"] = json!("0.51.0");
    v["guarantee"] = json!("certified");
    assert!(load(&v.to_string()).is_err());
}

#[test]
fn a_package_over_one_mebibyte_is_refused_before_parsing() {
    let big = format!("{{\"format\":\"{}\"}}", "x".repeat(1 << 20));
    assert!(matches!(load(&big), Err(GeneratorError::Limit { what: "bytes in a package", .. })));
}

#[test]
fn commands_that_reach_files_or_addresses_are_refused_at_load() {
    for command in [
        "file.open",
        "plugin.run",
        "generator.build",
        "settings.set",
        "sketch.import_svg",
        "sketch.insert_dxf",
        "component.insert_design",
        "engine.describe",
    ] {
        let text = package(json!([{"command": command, "params": {}}]));
        assert!(matches!(load(&text), Err(GeneratorError::Package(m)) if m.contains("may not run")), "{command}");
    }
    for key in ["filename", "File", "url", "folder"] {
        let text = package(json!([{"command": "solid.box", "params": {"nested": [{key: "x"}]}}]));
        assert!(matches!(load(&text), Err(GeneratorError::Package(m)) if m.contains("not allowed")), "{key}");
    }
}

fn repeat_step(count: &str, max: usize) -> Json {
    json!({"command": "solid.box", "repeat": {"var": "i", "count": count, "max": max}, "params": {"length": "${i}"}})
}

#[test]
fn hostile_repeat_counts_are_refused_before_any_expansion() {
    // A count of a billion, negative, fractional, not a number, over its max.
    for (count, ok_kind) in
        [("n", "limit"), ("-1", "expr"), ("1.5", "expr"), ("\"7\"", "expr"), ("0 / 0", "expr"), ("1e308 * 10", "expr"), ("true", "expr")]
    {
        let g = load(&package(json!([repeat_step(count, 10)]))).unwrap();
        let err = g.plan("X100000").unwrap_err();
        match (ok_kind, &err) {
            ("limit", GeneratorError::Limit { .. }) | ("expr", GeneratorError::Expr { .. }) => {}
            _ => panic!("count `{count}`: {err:?}"),
        }
    }
    // The same code with a small count builds.
    let g = load(&package(json!([repeat_step("n", 10)]))).unwrap();
    assert_eq!(g.plan("X3").unwrap().commands.len(), 3);
    assert_eq!(g.plan("X0").unwrap().commands.len(), 0);
}

#[test]
fn a_repeat_max_over_the_cap_is_refused_at_load() {
    assert!(matches!(load(&package(json!([repeat_step("1", 1025)]))), Err(GeneratorError::Package(_))));
    assert!(load(&package(json!([repeat_step("1", 1024)]))).is_ok());
}

#[test]
fn repeats_cannot_nest_and_cannot_multiply() {
    // A step that holds steps, or a repeat inside a repeat, is not in the format.
    let nested = json!([{"command": "solid.box", "repeat": {"var": "i", "count": "10", "max": 10}, "steps": [repeat_step("10", 10)]}]);
    assert!(matches!(load(&package(nested)), Err(GeneratorError::Package(_))));
    let inner = json!([{"command": "solid.box", "repeat": {"var": "i", "count": "10", "max": 10, "repeat": {"var": "j", "count": "10", "max": 10}}}]);
    assert!(matches!(load(&package(inner)), Err(GeneratorError::Package(_))));
}

#[test]
fn the_expanded_command_total_is_capped_across_steps() {
    // 2 x 300 repeats is under each step's max but over the 512 total, refused before running.
    let g = load(&package(json!([repeat_step("300", 300), repeat_step("300", 300)]))).unwrap();
    assert!(matches!(g.plan("X1"), Err(GeneratorError::Limit { what: "commands in one build", limit: 512 })));
    let one = load(&package(json!([repeat_step("512", 512)]))).unwrap();
    assert_eq!(one.plan("X1").unwrap().commands.len(), 512);
    let over = load(&package(json!([repeat_step("513", 600)]))).unwrap();
    assert!(matches!(over.plan("X1"), Err(GeneratorError::Limit { .. })));
}

#[test]
fn too_many_steps_and_oversized_parameters_are_refused() {
    let many: Vec<Json> = (0..257).map(|_| json!({"command": "solid.box"})).collect();
    assert!(matches!(load(&package(Json::Array(many))), Err(GeneratorError::Limit { what: "steps", .. })));
    // 600 KiB of text through one parameter.
    let g = load(&package(json!([{"command": "solid.box", "params": {"name": "${concat(\"a\", \"b\")}"}}]))).unwrap();
    assert!(g.plan("X1").is_ok());
    let big_list = json!({"name": "x".repeat(70_000)});
    let g = load(&package(json!([{"command": "solid.box", "params": big_list}]))).unwrap();
    assert!(matches!(g.plan("X1"), Err(GeneratorError::Limit { what: "bytes of one command's parameters", .. })));
}

fn with_table(csv: &str, ty: &str) -> String {
    let mut v: Json = serde_json::from_str(&package(json!([{"command": "solid.box", "params": {"points": "${t.p}"}}]))).unwrap();
    v["tables"] = json!({"t": {"columns": {"n": "number", "p": ty}, "key": ["n"], "csv": csv}});
    v["lookups"] = json!([{"table": "t", "as": "t", "key": {"n": "n"}}]);
    v.to_string()
}

#[test]
fn a_polygon_comes_from_a_table_cell_and_giant_ones_are_refused() {
    let g = load(&with_table("n,p\n1,0 0; 10 0; 10 5\n", "points")).unwrap();
    let plan = g.plan("X1").unwrap();
    assert_eq!(plan.commands[0].params["points"], json!([[0, 0], [10, 0], [10, 5]]));
    let ok: String = (0..512).map(|i| format!("{i} 0;")).collect();
    assert!(load(&with_table(&format!("n,p\n1,{ok}\n"), "points")).is_ok());
    let giant: String = (0..513).map(|i| format!("{i} 0;")).collect();
    assert!(matches!(load(&with_table(&format!("n,p\n1,{giant}\n"), "points")), Err(GeneratorError::Limit { what: "points in a table cell", .. })));
    for bad in ["1 2 3", "1", "a b", "1 inf", "1 NaN", "1e999 0"] {
        assert!(load(&with_table(&format!("n,p\n1,{bad}\n"), "points")).is_err(), "{bad}");
    }
}

#[test]
fn table_shapes_are_checked() {
    assert!(load(&with_table("n,p\n1,0 0\n1,1 1\n", "points")).is_err(), "repeated key");
    assert!(load(&with_table("n,p\nx,0 0\n", "points")).is_err(), "not a number");
    assert!(load(&with_table("n,p\n1\n", "points")).is_err(), "short row");
    assert!(load(&with_table("n,q\n1,0 0\n", "points")).is_err(), "undeclared header");
    assert!(load(&with_table("n,p,status\n1,0 0,bogus\n", "points")).is_err());
    assert!(load(&with_table("n,p\n1,\"0 0\n", "points")).is_err(), "open quote");
    let rows: String = (0..5001).map(|i| format!("{i},0 0\n")).collect();
    assert!(matches!(load(&with_table(&format!("n,p\n{rows}"), "points")), Err(GeneratorError::Limit { what: "table rows", .. })));
    let g = load(&with_table("n,p\n1,0 0\n", "points")).unwrap();
    assert!(matches!(g.parse("X2"), Err(GeneratorError::Refused { .. })), "a code with no row is refused");
}

#[test]
fn a_hostile_grammar_cannot_loop_or_blow_up() {
    // Many optional groups that all match nothing: the matching budget stops it.
    let opt = json!({"optional": [{"chars": "alpha", "field": "a", "min_len": 1, "max_len": 4}]});
    let alt: Vec<Json> = (0..30).map(|_| opt.clone()).collect();
    let mut v: Json = serde_json::from_str(&package(json!([{"command": "solid.box"}]))).unwrap();
    v["grammar"] = json!({"alternatives": [alt]});
    let g = load(&v.to_string()).unwrap();
    let r = g.parse(&"A".repeat(127));
    assert!(r.is_err());
    // Nested optionals deeper than 4 are refused at load.
    let mut deep = json!({"lit": "a"});
    for _ in 0..6 {
        deep = json!({"optional": [deep]});
    }
    v["grammar"] = json!({"alternatives": [[deep]]});
    assert!(load(&v.to_string()).is_err());
    // A token with two kinds, an unknown key and a bad range.
    for token in [
        json!({"lit": "a", "enum": ["b"], "field": "f"}),
        json!({"lit": "a", "extra": 1}),
        json!({"field": "f", "int": {"min": 5, "max": 1}}),
        json!({"field": "f", "int": {"min": 1, "max": 5, "step": 0}}),
        json!({"field": "bad name", "enum": ["a"]}),
        json!({"field": "f", "chars": "digits", "min_len": 0}),
        json!(7),
    ] {
        v["grammar"] = json!({"alternatives": [[token]]});
        assert!(load(&v.to_string()).is_err());
    }
}

#[test]
fn decimals_ranges_steps_and_flags_parse() {
    let mut v: Json = serde_json::from_str(&package(json!([{"command": "solid.box"}]))).unwrap();
    v["grammar"] = json!({"alternatives": [[
        {"lit": "W"},
        {"field": "w", "decimal": {"min": 0.5, "max": 9, "step": 0.5, "digits": 1}},
        {"field": "len", "int": {"min": 20, "max": 100, "step": 5}},
        {"flags": {"field": "opts", "options": [{"code": "-C", "value": "coated"}, {"code": "-B", "value": "black"}], "max": 2}}
    ]]});
    let g = load(&v.to_string()).unwrap();
    let fields = |code: &str| g.parse(code).map(|p| p.fields);
    let get = |f: &[(String, Value)], name: &str| f.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    let ok = fields("W2.540-C-B").unwrap();
    assert_eq!(get(&ok, "w"), Some(Value::Num(2.5)));
    assert_eq!(get(&ok, "len"), Some(Value::Num(40.0)));
    assert_eq!(get(&ok, "opts"), Some(Value::List(vec![Value::Text("coated".into()), Value::Text("black".into())])));
    assert!(fields("W2.3040").is_err(), "off the step");
    assert!(fields("W2.5042").is_err(), "off the length step");
    assert!(fields("W2.5105").is_err(), "over the maximum");
    assert!(fields("W2.540-C-C").is_err(), "a flag at most once");
    assert!(fields("W2.540-C-B-C").is_err(), "more flags than max");
    assert_eq!(get(&fields("W2.540").unwrap(), "opts"), Some(Value::List(vec![])));
}

#[test]
fn defaults_and_rules_use_other_fields_and_chain_lookups() {
    let mut v: Json = serde_json::from_str(&package(json!([{"command": "solid.box", "params": {"k": "${d}", "m": "${t2.v}"}}]))).unwrap();
    v["defaults"] = json!({"d": "n * 2"});
    v["tables"] = json!({
        "t1": {"columns": {"n": "number", "next": "text"}, "key": ["n"], "csv": "n,next\n3,a\n"},
        "t2": {"columns": {"k": "text", "v": "number"}, "key": ["k"], "csv": "k,v\na,42\n"}
    });
    v["lookups"] = json!([{"table": "t1", "as": "t1", "key": {"n": "n"}}, {"table": "t2", "as": "t2", "key": {"k": "t1.next"}}]);
    v["rules"] = json!([{"when": "d > 100", "refuse": "{d} is too big for X{n}"}]);
    let g = load(&v.to_string()).unwrap();
    let plan = g.plan("X3").unwrap();
    assert_eq!(plan.commands[0].params, json!({"k": 6, "m": 42}));
    assert_eq!(plan.parsed.rows.len(), 2);
    v["tables"]["t1"]["csv"] = json!("n,next\n3,a\n60,a\n");
    let g = load(&v.to_string()).unwrap();
    assert!(matches!(g.parse("X60"), Err(GeneratorError::Refused { message, .. }) if message == "120 is too big for X60"));
}

#[test]
fn a_rule_that_is_not_a_boolean_or_has_a_cycle_style_forward_reference_is_an_error() {
    let mut v: Json = serde_json::from_str(&package(json!([{"command": "solid.box"}]))).unwrap();
    v["vars"] = json!([{"name": "a", "expr": "b + 1"}, {"name": "b", "expr": "1"}]);
    let g = load(&v.to_string()).unwrap();
    assert!(matches!(g.parse("X1"), Err(GeneratorError::Expr { message, .. }) if message.contains("unknown name")));
    v["vars"] = json!([{"name": "a", "expr": "1"}, {"name": "a", "expr": "2"}]);
    assert!(load(&v.to_string()).is_err(), "a repeated variable name");
    v["vars"] = json!([]);
    v["rules"] = json!([{"when": "n + 1", "refuse": "no"}]);
    let g = load(&v.to_string()).unwrap();
    assert!(matches!(g.parse("X1"), Err(GeneratorError::Expr { .. })));
    v["rules"] = json!([{"when": "1 / 0 > 1", "refuse": "no"}]);
    let g = load(&v.to_string()).unwrap();
    assert!(matches!(g.parse("X1"), Err(GeneratorError::Expr { .. })));
}

#[test]
fn a_whole_build_that_spends_its_steps_is_stopped() {
    // 512 commands, each rendering many expression nodes, stays inside the 1,000,000 steps;
    // a deliberately small budget is what the typed error is tested with (expression tests).
    let g = load(&package(json!([repeat_step("512", 512)]))).unwrap();
    assert!(g.plan("X1").is_ok());
}

#[test]
fn error_messages_read_as_sentences() {
    let e = GeneratorError::Limit { what: "table rows", limit: 5000 };
    assert_eq!(e.to_string(), "generator limit: more than 5000 table rows");
    let e = GeneratorError::NeedsNewer { needs: "0.60.0".into(), have: "0.51.0".into() };
    assert!(e.to_string().contains("needs SolidCraft 0.60.0"));
}

// ---- the additions for the first adopter (docs/formats/generator-package.md, section 11) ----

/// A package with a table `t` (key `k`), a lookup `t` on the field `k`, and the given extras.
fn with(extra: Json) -> Generator {
    let mut v: Json = json!({
        "format": "solidcraft-generator/1", "id": "extras-test", "name": "Extras",
        "grammar": {"alternatives": [[{"field": "k", "enum": ["a1", "b2", "c3"], "nocase": true}]]},
        "tables": {"t": {"columns": {"k": "text", "n": "number"}, "key": ["k"], "csv": "k,n\na1,5\nb2,7\n"}},
        "lookups": [{"table": "t", "as": "t", "key": {"k": "k"}}],
        "steps": [{"command": "solid.box", "params": {"length": "${t.n}", "width": 1, "height": 1}}]
    });
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    Generator::load(&v.to_string(), NOW).unwrap()
}

fn refusal(g: &Generator, code: &str) -> (RefusalKind, String) {
    match g.parse(code) {
        Err(GeneratorError::Refused { kind, message }) => (kind, message),
        other => panic!("{code}: {other:?}"),
    }
}

#[test]
fn an_early_rule_and_a_missing_kind_refuse_a_valid_but_undrawn_type_as_not_modelled() {
    // c3 is a real code with no table row: the lookup refuses it, with the kind the package gives.
    let g = with(json!({}));
    assert_eq!(refusal(&g, "c3").0, RefusalKind::Invalid);
    let mut v = json!([{"table": "t", "as": "t", "key": {"k": "k"}, "missing": "{k} is not drawn", "missing_kind": "not_modelled"}]);
    let g = with(json!({"lookups": v.clone()}));
    assert_eq!(refusal(&g, "c3"), (RefusalKind::NotModelled, "c3 is not drawn".into()));
    // An early rule runs before the lookups, so it can answer for a type that has no row at all.
    v = json!([{"when": "k == \"c3\"", "refuse": "{k} is a valid code, not drawn here", "kind": "not_modelled", "phase": "early"}]);
    let g = with(json!({"rules": v}));
    assert_eq!(refusal(&g, "c3"), (RefusalKind::NotModelled, "c3 is a valid code, not drawn here".into()));
    // ...and cannot read a lookup, which does not exist yet.
    let g = with(json!({"rules": [{"when": "t.n > 1", "refuse": "x", "phase": "early"}]}));
    assert!(matches!(g.parse("a1"), Err(GeneratorError::Expr { .. })));
    // Bad words are refused at load.
    for bad in [
        json!({"lookups": [{"table": "t", "as": "t", "key": {"k": "k"}, "missing_kind": "gone"}]}),
        json!({"rules": [{"when": "true", "refuse": "x", "phase": "soon"}]}),
    ] {
        let mut v: Json = serde_json::from_str(EXAMPLE).unwrap();
        for (k, x) in bad.as_object().unwrap() {
            v[k] = x.clone();
        }
        assert!(load(&v.to_string()).is_err());
    }
}

#[test]
fn case_folding_is_per_enum_or_for_the_whole_code() {
    let g = with(json!({}));
    assert_eq!(g.parse("B2").unwrap().code, "B2");
    let g = with(
        json!({"grammar": {"alternatives": [[{"lit": "x"}, {"field": "k", "chars": "alnum", "min_len": 1, "max_len": 4}]], "normalize": "lower", "refuse": "{code} is wrong"}}),
    );
    let parsed = g.parse("XA1").unwrap();
    assert_eq!(parsed.code, "xa1");
    assert_eq!(parsed.fields[0].1, Value::Text("a1".into()));
    // The message shows the code as it was typed.
    assert_eq!(refusal(&g, "ZZ9").1, "ZZ9 is wrong");
    // Upper: the folded code is what is looked up.
    let g = with(
        json!({"grammar": {"alternatives": [[{"lit": "X"}, {"field": "k", "chars": "alnum", "min_len": 1, "max_len": 4}]], "normalize": "upper"}}),
    );
    assert!(refusal(&g, "xa1").1.contains("A1"));
    let mut v: Json = serde_json::from_str(EXAMPLE).unwrap();
    v["grammar"]["normalize"] = json!("title");
    assert!(load(&v.to_string()).is_err());
}

#[test]
fn numbers_become_text_in_one_defined_way() {
    assert_eq!(ev("fmt(2.5)").unwrap(), Value::Text("2.5".into()));
    assert_eq!(ev("fmt(1000)").unwrap(), Value::Text("1000".into()));
    assert_eq!(ev("fmt(497.0)").unwrap(), Value::Text("497".into()));
    assert_eq!(ev("fmt(4.2)").unwrap(), Value::Text("4.2".into()));
    assert_eq!(ev("fmt(0.1 + 0.2)").unwrap(), Value::Text("0.30000000000000004".into()));
    assert_eq!(ev("fmt(1.5, 3)").unwrap(), Value::Text("1.500".into()));
    assert_eq!(ev("concat(\"M\", 3.5, \"-\", 4)").unwrap(), Value::Text("M3.5-4".into()));
    assert_eq!(ev("\"x\" + -0").unwrap(), Value::Text("x0".into()));
}

#[test]
fn empty_cells_mean_no_items_or_no_value() {
    let csv = "k,pts,lst,opt,nums,groups\na1,,,,,\nb2,0 0; 1 1,x y,3.5,1 2 3,1 2; 3 4 5\n";
    let make = |ty: &str| {
        let mut v: Json = json!({
            "format": "solidcraft-generator/1", "id": "cells-test", "name": "Cells",
            "grammar": {"alternatives": [[{"field": "k", "enum": ["a1", "b2"]}]]},
            "tables": {"t": {"columns": {"k": "text", "pts": "points", "lst": "list", "opt": ty, "nums": "numbers", "groups": "tuples"}, "key": ["k"], "csv": csv}},
            "lookups": [{"table": "t", "as": "t", "key": {"k": "k"}}],
            "vars": [{"name": "has_opt", "expr": "has(t.opt)"}, {"name": "n_pts", "expr": "len(t.pts)"}, {"name": "n_groups", "expr": "len(t.groups)"}, {"name": "third", "expr": "len(t.nums) > 2 ? at(t.nums, 2) : 0"}],
            "steps": [{"command": "solid.box", "params": {"pts": "${t.pts}", "lst": "${t.lst}", "nums": "${t.nums}", "groups": "${t.groups}", "opt": "${t.opt}"}}]
        });
        v["x-note"] = json!(1);
        load(&v.to_string())
    };
    // An empty cell is legal for a list, points, numbers or tuples column, and for `number?`.
    let g = make("number?").unwrap();
    let a = g.plan("a1").unwrap();
    assert_eq!(a.commands[0].params, json!({"pts": [], "lst": [], "nums": [], "groups": [], "opt": null}));
    assert_eq!(Generator::named_value(&a.parsed, "n_pts"), Some(0.0));
    assert_eq!(a.parsed.vars[0].1, Value::Bool(false));
    let b = g.plan("b2").unwrap();
    assert_eq!(
        b.commands[0].params,
        json!({"pts": [[0, 0], [1, 1]], "lst": ["x", "y"], "nums": [1, 2, 3], "groups": [[1, 2], [3, 4, 5]], "opt": 3.5})
    );
    assert_eq!(b.parsed.vars[0].1, Value::Bool(true));
    assert_eq!(Generator::named_value(&b.parsed, "third"), Some(3.0));
    assert_eq!(Generator::named_value(&b.parsed, "n_groups"), Some(2.0));
    // A plain `number` column still wants a number.
    assert!(make("number").is_err());
    // Arithmetic on an empty value is an error, not a zero.
    let mut scope = Scope::new();
    scope.insert("x".into(), Value::None);
    let e = expr::parse("x + 1").unwrap();
    assert!(expr::eval(&e, &scope, &mut Budget { remaining: 100 }).is_err());
}

#[test]
fn a_table_list_drives_one_repeat_and_a_block_keeps_its_commands_in_order() {
    let mut v: Json = json!({
        "format": "solidcraft-generator/1", "id": "block-test", "name": "Block",
        "grammar": {"alternatives": [[{"field": "k", "enum": ["a1"]}]]},
        "tables": {"t": {"columns": {"k": "text", "bores": "tuples"}, "key": ["k"], "csv": "k,bores\na1,1 2 3; 4 5 6; 7 8 9\n"}},
        "lookups": [{"table": "t", "as": "t", "key": {"k": "k"}}],
        "steps": [{
            "repeat": {"var": "i", "count": "len(t.bores)", "max": 5},
            "steps": [
                {"command": "sketch.line", "params": {"x": "${at(at(t.bores, i), 0)}", "n": "${i}"}},
                {"command": "sketch.arc.three_point", "when": "i < 2", "params": {"d": "${at(at(t.bores, i), 2)}", "n": "${i}"}}
            ]
        }]
    });
    let g = load(&v.to_string()).unwrap();
    let plan = g.plan("a1").unwrap();
    let order: Vec<(String, i64)> = plan.commands.iter().map(|c| (c.command.clone(), c.params["n"].as_i64().unwrap())).collect();
    assert_eq!(
        order,
        [("sketch.line", 0), ("sketch.arc.three_point", 0), ("sketch.line", 1), ("sketch.arc.three_point", 1), ("sketch.line", 2)]
            .map(|(c, n)| (c.to_string(), n))
    );
    assert_eq!(plan.commands[0].params["x"], json!(1));
    assert_eq!(plan.commands[3].params["d"], json!(6));
    // A block cannot hold a repeat or a block of its own, and needs commands.
    for bad in [
        json!([{"steps": [{"command": "solid.box", "repeat": {"var": "j", "count": "2", "max": 2}}]}]),
        json!([{"steps": [{"steps": [{"command": "solid.box"}]}]}]),
        json!([{"steps": []}]),
        json!([{"command": "solid.box", "steps": [{"command": "solid.box"}]}]),
        json!([{"steps": [{"command": "solid.box"}], "params": {}}]),
        json!([{"params": {}}]),
    ] {
        v["steps"] = bad;
        assert!(load(&v.to_string()).is_err());
    }
    // Blocks multiply, so the total is counted before anything is expanded: 20 commands x 30.
    let twenty: Vec<Json> = (0..20).map(|_| json!({"command": "solid.box"})).collect();
    v["steps"] = json!([{"repeat": {"var": "i", "count": "30", "max": 30}, "steps": twenty}]);
    let g = load(&v.to_string()).unwrap();
    assert!(matches!(g.plan("a1"), Err(GeneratorError::Limit { what: "commands in one build", limit: 512 })));
    // The template as a whole is capped too.
    let many: Vec<Json> = (0..60).map(|_| json!({"command": "solid.box"})).collect();
    v["steps"] = Json::Array((0..5).map(|_| json!({"steps": many.clone()})).collect());
    assert!(matches!(load(&v.to_string()), Err(GeneratorError::Limit { what: "commands in the template", .. })));
}

#[test]
fn constants_are_names_for_every_expression_and_may_key_a_lookup() {
    let mut v: Json = json!({
        "format": "solidcraft-generator/1", "id": "const-test", "name": "Const",
        "const": {"plan": "p", "ratio": 0.5, "on": true},
        "grammar": {"alternatives": [[{"field": "k", "enum": ["a1"]}]]},
        "tables": {"params": {"columns": {"name": "text", "value": "number"}, "key": ["name"], "csv": "name,value\np,10\n"}},
        "lookups": [{"table": "params", "as": "pr", "key": {"name": "plan"}}],
        "vars": [{"name": "half", "expr": "pr.value * ratio"}],
        "steps": [{"command": "solid.box", "when": "on", "params": {"length": "${half}"}}]
    });
    let plan = load(&v.to_string()).unwrap().plan("a1").unwrap();
    assert_eq!(plan.commands[0].params["length"], json!(5));
    // A constant is not a field: it is not in the fields shown.
    assert!(plan.parsed.fields.iter().all(|(n, _)| n != "plan" && n != "ratio"));
    for bad in [json!({"k": 1}), json!({"code": 1}), json!({"half": 1}), json!({"x y": 1}), json!({"list": [1]}), json!({"big": 1e999})] {
        v["const"] = bad;
        assert!(load(&v.to_string()).is_err());
    }
}

#[test]
fn the_allow_list_has_metadata_and_visibility_and_a_package_may_skip_the_component() {
    let ok = ["browser.visibility", "component.metadata.set", "document.metadata.set", "solid.thread", "sketch.line", "construct.plane.offset"];
    let steps: Vec<Json> = ok.iter().map(|c| json!({"command": c})).collect();
    let mut v: Json = serde_json::from_str(&package(json!([{"command": "solid.box"}]))).unwrap();
    v["steps"] = Json::Array(steps);
    v["insert"] = json!({"component": false});
    let plan = load(&v.to_string()).unwrap().plan("X1").unwrap();
    assert_eq!(plan.component, None);
    assert_eq!(plan.commands.len(), ok.len());
    for no in ["browser.set_visibility_file", "document.open", "document.save", "settings.set", "file.save", "ui.start"] {
        v["steps"] = json!([{"command": no}]);
        assert!(load(&v.to_string()).is_err(), "{no}");
    }
    v["steps"] = json!([{"command": "solid.box"}]);
    v["insert"] = json!({"component": 5});
    assert!(load(&v.to_string()).is_err());
}

#[test]
fn a_row_can_give_a_status_per_column() {
    let csv = "k,a,b,c,status\na1,1,2,3,\"measured:a,b;approximate:c;published\"\nb2,1,2,3,labelled\n";
    let mut v: Json = json!({
        "format": "solidcraft-generator/1", "id": "status-test", "name": "Status",
        "grammar": {"alternatives": [[{"field": "k", "enum": ["a1", "b2"]}]]},
        "tables": {"t": {"columns": {"k": "text", "a": "number", "b": "number", "c": "number", "status": "text"}, "key": ["k"], "csv": csv}},
        "lookups": [{"table": "t", "as": "t", "key": {"k": "k"}}],
        "steps": [{"command": "solid.box"}]
    });
    let g = load(&v.to_string()).unwrap();
    let row = &g.parse("a1").unwrap().rows[0];
    assert_eq!(row.status.as_deref(), Some("published"));
    assert_eq!(row.column_status.get("a").map(String::as_str), Some("measured"));
    assert_eq!(row.column_status.get("b").map(String::as_str), Some("measured"));
    assert_eq!(row.column_status.get("c").map(String::as_str), Some("approximate"));
    let row = &g.parse("b2").unwrap().rows[0];
    assert_eq!((row.status.as_deref(), row.column_status.len()), (Some("labelled"), 0));
    for bad in ["measured:zz", "guessed", "measured:a;whatever"] {
        v["tables"]["t"]["csv"] = json!(format!("k,a,b,c,status\na1,1,2,3,\"{bad}\"\n"));
        assert!(load(&v.to_string()).is_err(), "{bad}");
    }
}

#[test]
fn the_review_object_is_checked_and_x_keys_are_allowed_inside_every_object() {
    let mut v: Json = serde_json::from_str(EXAMPLE).unwrap();
    v["review"] = json!({"reviewer": "A. Reader", "reviewer_kind": "person", "date": "2026-11-27", "sources_hash": "sha256:00", "level": "reviewed", "notes": "ok", "x-extra": 1});
    let g = load(&v.to_string()).unwrap();
    let review = g.meta.review.as_ref().unwrap();
    assert_eq!((review.reviewer_kind.as_deref(), review.date.as_str(), review.level.as_deref()), (Some("person"), "2026-11-27", Some("reviewed")));
    for bad in [
        json!({"reviewer_kind": "robot"}),
        json!({"date": "27 Nov"}),
        json!({"level": "great"}),
        json!({"surprise": 1}),
        json!({"reviewer": "x".repeat(300)}),
    ] {
        v["review"] = bad;
        assert!(load(&v.to_string()).is_err());
    }
    v.as_object_mut().unwrap().remove("review");
    // x- keys on a step, a block, a repeat, a table, a lookup, a rule, a variable, a source, a grammar and a token.
    v["steps"][0]["x-why"] = json!("note");
    v["steps"][1]["repeat"]["x-why"] = json!("note");
    v["tables"]["size"]["x-why"] = json!("note");
    v["lookups"][0]["x-why"] = json!("note");
    v["rules"][0]["x-why"] = json!("note");
    v["vars"][0]["x-why"] = json!("note");
    v["sources"][0]["x-why"] = json!("note");
    v["grammar"]["x-why"] = json!("note");
    v["grammar"]["alternatives"][0][0]["x-why"] = json!("note");
    v["checks"][0]["x-why"] = json!("note");
    assert!(load(&v.to_string()).is_ok());
    // A misspelt key without the x- is still an error.
    v["steps"][0]["whhy"] = json!(1);
    assert!(load(&v.to_string()).is_err());
}
