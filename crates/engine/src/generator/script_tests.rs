//! The generator script sandbox: what a script can do, and that every runaway ends cleanly.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;

use super::code_template::{Declared, Params, Template};
use super::csv_table::Table;
use super::script::{RefusalKind, Script, ScriptError};

fn script(src: &str) -> Result<Script, ScriptError> {
    let template = Template::parse("MGN{size: int 7..15}R{length: int 40..2000}", &Declared::new()).unwrap();
    let mut tables = BTreeMap::new();
    tables.insert("rows".to_string(), Table::parse("key,pitch,note\nc15,60,standard\nh15,60,\n").unwrap());
    Script::compile(src, Arc::new(vec![template]), Arc::new(tables))
}

fn fields(v: serde_json::Value) -> Params {
    v.as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

#[test]
fn resolve_returns_data_and_can_use_table_and_code_template() {
    let s = script(
        r#"
        fn resolve(f) {
            let row = table("rows", "c" + f.size);
            if row == () { refuse("not_modelled", "no row for size " + f.size); }
            #{ configuration: "C-" + f.size, parameters: #{ length: f.length.to_string() + " mm", pitch: row.pitch },
               code: code_template(#{ size: f.size, length: f.length }), pitch: table("rows", "h15", "pitch") }
        }
        "#,
    )
    .unwrap();
    let out = s.resolve(&fields(json!({"size": 15, "length": 600}))).unwrap().unwrap();
    assert_eq!(out["configuration"], "C-15");
    assert_eq!(out["parameters"], json!({"length": "600 mm", "pitch": 60}));
    assert_eq!(out["code"], "MGN15R600");
    assert_eq!(out["pitch"], 60);
    let e = s.resolve(&fields(json!({"size": 9, "length": 100}))).unwrap_err();
    assert_eq!(e, ScriptError::Refused { kind: RefusalKind::NotModelled, message: "no row for size 9".into() });
}

#[test]
fn a_script_without_resolve_gives_none_and_a_throw_is_a_script_error() {
    assert_eq!(script("fn other(x) { x }").unwrap().resolve(&Params::new()).unwrap(), None);
    let s = script(r#"fn resolve(f) { throw "bad row"; }"#).unwrap();
    assert_eq!(s.resolve(&Params::new()).unwrap_err(), ScriptError::Script("bad row".into()));
    let s = script(r#"fn resolve(f) { refuse("nope", "x") }"#).unwrap();
    assert!(matches!(s.resolve(&Params::new()), Err(ScriptError::Script(_))));
}

#[test]
fn runaway_scripts_stop_with_a_limit_not_a_hang() {
    for src in [
        "fn resolve(f) { loop { } }",
        "fn resolve(f) { let i = 0; while true { i += 1; } }",
        "fn resolve(f) { resolve(f) }",
        r#"fn resolve(f) { let s = "x"; loop { s += s; } }"#,
        "fn resolve(f) { let a = []; loop { a.push(1); } }",
        "fn resolve(f) { let m = #{}; let i = 0; loop { m[i.to_string()] = i; i += 1; } }",
    ] {
        let s = script(src).unwrap();
        let e = s.resolve(&Params::new()).unwrap_err();
        assert!(matches!(e, ScriptError::Limit(_)), "{src}: {e:?}");
    }
}

#[test]
fn deep_expressions_and_oversized_scripts_are_refused_when_compiled() {
    let deep = format!("fn resolve(f) {{ {}1{} }}", "(".repeat(200), ")".repeat(200));
    assert!(script(&deep).is_err());
    let big = format!("fn resolve(f) {{ 1 }} // {}", "x".repeat(40 * 1024));
    assert!(matches!(script(&big), Err(ScriptError::Limit(_))));
    assert!(script("fn resolve(f) {").is_err());
}

#[test]
fn a_script_cannot_reach_files_modules_eval_or_the_clock() {
    for src in [
        r#"import "other" as o; fn resolve(f) { 1 }"#,
        r#"fn resolve(f) { eval("1 + 1") }"#,
        r#"fn resolve(f) { timestamp() }"#,
        r#"fn resolve(f) { open("/etc/passwd") }"#,
    ] {
        match script(src) {
            Err(_) => {}
            Ok(s) => assert!(s.resolve(&Params::new()).is_err(), "{src}"),
        }
    }
}

#[test]
fn describe_returns_text_or_nothing() {
    let s = script(r#"fn describe(p) { "rail " + p.length }"#).unwrap();
    assert_eq!(s.describe(&fields(json!({"length": 600}))).unwrap(), Some("rail 600".into()));
    assert_eq!(script("fn resolve(f) { 1 }").unwrap().describe(&Params::new()).unwrap(), None);
    assert!(script("fn describe(p) { 5 }").unwrap().describe(&Params::new()).is_err());
}

#[test]
fn tables_read_numbers_text_and_empty_cells_and_refuse_bad_files() {
    let t = Table::parse("key,a,b\nx,1,\"q,r\"\nY,2.5,007\n").unwrap();
    assert_eq!(t.row("y").unwrap()[1].1, &json!(2.5));
    assert_eq!(t.row("x").unwrap()[2].1, &json!("q,r"));
    assert_eq!(t.row("Y").unwrap()[2].1, &json!("007"));
    for bad in ["", "key,key\n1,2\n", "key,a\n1\n", "key,a\nx,1\nx,2\n", "key,a\n,1\n", "1key,a\nx,1\n", "key,a\nx,\"1\n"] {
        assert!(Table::parse(bad).is_err(), "{bad:?}");
    }
    let many = format!("key\n{}", (0..5001).map(|i| format!("{i}\n")).collect::<String>());
    assert!(Table::parse(&many).is_err());
}

#[test]
fn csv_keys_keep_their_original_decimal_spelling() {
    let t = Table::parse("key,status\n1.50,not_modelled\nABC,needs_review\n").unwrap();
    assert_eq!(t.row("1.50").unwrap()[1].1, &json!("not_modelled"));
    assert!(t.row("1.5").is_none());
    assert_eq!(t.row("abc").unwrap()[1].1, &json!("needs_review"));
}
