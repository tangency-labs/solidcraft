#![allow(clippy::unwrap_used, clippy::panic)]
//! #11597: offsetting a chain of an arc, a line and a spline.
use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|error| panic!("{command} {params}: {error}"))
}

/// An arc, a line and a control-point spline meeting end to end (the line leaves the arc's end
/// and the spline leaves the line's end going the same way).
fn chain() -> (Session, Vec<String>) {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let mut ids = Vec::new();
    let mut id_of = |v: Value| ids.push(v["curves"][0].as_str().unwrap().to_string());
    id_of(run(
        &mut s,
        "sketch.arc.three_point",
        json!({"start": [27.057152168740334, 4.6392170815647855], "through": [29.1011788575, 6.683243770299466], "end": [27.057152168731445, 8.7272704590365]}),
    ));
    id_of(run(
        &mut s,
        "sketch.line",
        json!({"points": [[27.057152168728745, 8.72727045903728], [3.3203869526962912, 8.727270459038918]], "connect": false}),
    ));
    id_of(run(
        &mut s,
        "sketch.spline.control_point",
        json!({"connect": false, "points": [[3.320386952696254, 8.727270459038918], [1.8337408439045355, 8.7272704590], [1.2130077051, 10.6284037520], [1.6440729637, 13.3010122908], [3.3203869527, 16.7840253217], [6.0690326225, 19.6015032445], [9.8223501930, 21.9267287781], [14.9488315292, 22.8085245156], [16.7566174523, 22.8085245156]]}),
    ));
    (s, ids)
}

#[test]
fn offsetting_an_arc_line_and_spline_chain_works() {
    for (all, chain_on) in [(true, true), (true, false), (false, true)] {
        for distance in [1.0, 0.5, -1.0] {
            let (mut s, ids) = chain();
            let curves = if all { json!(ids) } else { json!([ids[0]]) };
            let r = s.execute("sketch.offset", &json!({"curves": curves, "distance": distance, "chain": chain_on}));
            assert!(r.is_ok(), "{curves} chain {chain_on} distance {distance}: {:?}", r.err());
        }
    }
}

/// The 1 mm chain offset is the right curve, not just some curve: the chain's two ends are offset
/// by exactly 1 mm. On main this fails with "offset source has a stationary tangent" (#11597).
#[test]
fn the_chain_offset_ends_lie_one_millimetre_from_the_source_ends() {
    let (mut s, ids) = chain();
    let r = run(&mut s, "sketch.offset", json!({"curves": ids, "distance": 1.0, "chain": true}));
    let curves = r["curves"].as_array().unwrap();
    assert_eq!(curves.len(), 1, "the chain offsets to one curve: {curves:?}");
    let sk = s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone();
    let ends: Vec<_> = curves
        .iter()
        .flat_map(|c| {
            let k = &sk.curves[sk.curve_index(c.as_str().unwrap()).unwrap()].kind;
            let (a, b) = k.ends().unwrap();
            [sk.points[a].pos, sk.points[b].pos]
        })
        .collect();
    let near = |x: f64, y: f64| ends.iter().any(|p| (((p.x - x).powi(2) + (p.y - y).powi(2)).sqrt() - 1.0).abs() < 1e-6);
    assert!(near(27.057152168740334, 4.6392170815647855), "the arc's free end: {ends:?}");
    assert!(near(16.7566174523, 22.8085245156), "the spline's free end: {ends:?}");
}
