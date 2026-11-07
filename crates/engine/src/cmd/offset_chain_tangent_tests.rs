#![allow(clippy::unwrap_used, clippy::panic)]
//! #11597: offsetting a chain of an arc, a line and a spline.
use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|error| panic!("{command} {params}: {error}"))
}

/// An arc, a line and a control-point spline meeting end to end (the line leaves the arc's end
/// and the spline leaves the line's end going the same way).
fn chain() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.arc.three_point", json!({"start": [27.0571521687, 4.6392170816], "through": [29.1011788575, 6.6832437703], "end": [27.0571521687, 8.7272704590], "ids": ["a2"]}));
    run(&mut s, "sketch.line", json!({"points": [[27.0571521687, 8.7272704590], [3.3203869527, 8.7272704590]], "ids": ["l15"]}));
    run(
        &mut s,
        "sketch.spline.control_point",
        json!({"points": [[3.3203869527, 8.7272704590], [1.8337408440, 8.7272704590], [1.2130077051, 10.6284037520], [1.6440729637, 13.3010122908], [3.3203869527, 16.7840253217], [6.0690326225, 19.6015032445], [9.8223501930, 21.9267287781], [14.9488315292, 22.8085245156], [16.7566174523, 22.8085245156]], "ids": ["s3"]}),
    );
    s
}

#[test]
fn offsetting_an_arc_line_and_spline_chain_works() {
    for (curves, chain_on) in [(json!(["a2", "l15", "s3"]), true), (json!(["a2", "l15", "s3"]), false), (json!(["a2"]), true)] {
        for distance in [1.0, 0.5, -1.0] {
            let mut s = chain();
            let r = s.execute("sketch.offset", &json!({"curves": curves, "distance": distance, "chain": chain_on}));
            assert!(r.is_ok(), "{curves} chain {chain_on} distance {distance}: {:?}", r.err());
        }
    }
}
