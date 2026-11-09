use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{PatchResult, SurfacePatchParams};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<SurfacePatchParams>(),
        &["body", "body_name", "body_names", "continuity", "crown", "edges", "exact_regions", "name", "participants", "points", "profiles", "sketch"],
    );
}

/// Eight points round the open top of [`tube`]'s `Tube`, a closed loop of its edges.
fn rim() -> Value {
    json!((0..8).map(|k| f64::from(k) * 0.785).map(|a| [10.0 * a.cos(), 10.0 * a.sin(), 30.0]).collect::<Vec<_>>())
}

/// At `level`: a cylinder `Tube` (radius 10, height 30) with its top face taken off, so its rim
/// is a loop of edges to patch, and a sketch `S` beside it with one rectangle, a region to patch.
fn tube(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    run("solid.cylinder", json!({"radius": 10, "height": 30, "body_name": "Tube"}));
    run("surface.unstitch", json!({"body": "Tube", "faces": [[0, 0, 30]]}));
    run("sketch.create", json!({"plane": "XY", "name": "S"}));
    run("sketch.rectangle.two_point", json!({"p0": [30, 0], "p1": [40, 10]}));
    run("sketch.finish", json!({}));
    s
}

/// `{edges: rim, body: Tube}` with `extra`'s keys added.
fn on_rim(extra: Value) -> Value {
    let mut p = json!({"edges": rim(), "body": "Tube"});
    if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
        p.extend(extra.clone());
    }
    p
}

/// Calls that fail today give the same message in strict, each on a fresh [`tube`]: what the
/// contract checks (the crown, each form of continuity, the points, the edges' count, the sketch
/// and profiles) and what it leaves to the handler (a body or sketch the design lacks, points and
/// edges off the loop, a profile the sketch lacks, and continuity or points on a sketch patch).
#[test]
fn strict_keeps_todays_errors() {
    let cases = [
        on_rim(json!({"crown": "big"})),
        on_rim(json!({"crown": 5})),
        on_rim(json!({"crown": -1})),
        on_rim(json!({"crown": null})),
        on_rim(json!({"continuity": "g5"})),
        on_rim(json!({"continuity": " G9 "})),
        on_rim(json!({"continuity": "smooth"})),
        on_rim(json!({"continuity": 5})),
        on_rim(json!({"continuity": null})),
        on_rim(json!({"continuity": true})),
        on_rim(json!({"continuity": [{"edge": "here", "g": "g1"}]})),
        on_rim(json!({"continuity": [{"edge": [10, 0, 30]}]})),
        on_rim(json!({"continuity": [5]})),
        on_rim(json!({"continuity": vec![json!({"edge": [10, 0, 30], "g": "g1"}); 10_001]})),
        on_rim(json!({"continuity": [{"edge": [0, 0, 0], "g": "g1"}]})),
        on_rim(json!({"continuity": {"all": "g9"}})),
        on_rim(json!({"continuity": {"all": null}})),
        on_rim(json!({"continuity": {"edges": 5}})),
        on_rim(json!({"continuity": {"all": "g1", "edges": [{"edge": [10, 0, 30], "g": "g7"}]}})),
        on_rim(json!({"points": "up"})),
        on_rim(json!({"points": null})),
        on_rim(json!({"points": vec![[0, 0, 35]; 65]})),
        on_rim(json!({"points": [[0, 0, "x"]]})),
        on_rim(json!({"points": [[100, 0, 30]]})),
        on_rim(json!({"edges": vec![[10, 0, 30]; 10_001]})),
        on_rim(json!({"edges": [[500, 500, 500], [500, 510, 500], [510, 500, 500]]})),
        on_rim(json!({"body": "Nope"})),
        json!({"sketch": "S", "continuity": "g1"}),
        json!({"sketch": "S", "points": [[35, 5, 0]]}),
        json!({"sketch": "Nope"}),
        json!({"sketch": true}),
        json!({"sketch": null}),
        json!({"sketch": "S", "profiles": "some"}),
        json!({"sketch": "S", "profiles": null}),
        json!({"sketch": "S", "profiles": [7]}),
    ];
    for p in cases {
        let error = |level: ParamCheck| tube(level).execute("surface.patch", &p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// #13434: the contract takes what its input schema allows: each call's check finds no issue
/// exactly when the call matches the schema. Continuity words are in lower case here: the check
/// also takes capitals, as the handler does, and the schema's enum doesn't. A sketch patch with
/// continuity matches the schema; the handler refuses it.
#[test]
fn the_check_takes_what_the_schema_allows() {
    let schema = input_schema_for::<SurfacePatchParams>();
    let schema = schema.as_value();
    let edge = |extra: Value| {
        let mut p = json!({"edges": [[1, 2, 3]], "body": "B"});
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        p
    };
    for (p, takes) in [
        (edge(json!({})), true),
        (edge(json!({"edges": [{"x": 1, "y": 2, "z": 3}], "body": null, "name": "Lid", "body_name": "Lid"})), true),
        (edge(json!({"edges": []})), true),
        (edge(json!({"edges": null})), true),
        (edge(json!({"continuity": "g4", "crown": -0.9})), true),
        (edge(json!({"continuity": [{"edge": [1, 2, 3], "g": "g2"}], "crown": 3})), true),
        (edge(json!({"continuity": {"all": "g1", "edges": [{"edge": [1, 2, 3], "g": "g3", "note": 1}]}})), true),
        (edge(json!({"continuity": {}, "points": []})), true),
        (edge(json!({"continuity": {"all": "g1", "note": 1}, "points": [[1, 2, 3]]})), true),
        (json!({"sketch": "S", "profiles": "all", "exact_regions": true, "body_names": ["A"], "participants": ["*"]}), true),
        (json!({"sketch": 3, "profiles": [0, 1], "continuity": "g1"}), true),
        (json!({}), true),
        (edge(json!({"edges": 5})), false),
        (edge(json!({"edges": [[1, 2]]})), false),
        (edge(json!({"edges": [[1, 2, 3], "x"]})), false),
        (edge(json!({"body": 5})), false),
        (edge(json!({"crown": 3.5})), false),
        (edge(json!({"crown": -1})), false),
        (edge(json!({"crown": "1"})), false),
        (edge(json!({"crown": null})), false),
        (edge(json!({"continuity": "g5"})), false),
        (edge(json!({"continuity": 5})), false),
        (edge(json!({"continuity": null})), false),
        (edge(json!({"continuity": [{"edge": [1, 2, 3]}]})), false),
        (edge(json!({"continuity": [{"g": "g1"}]})), false),
        (edge(json!({"continuity": {"all": null}})), false),
        (edge(json!({"continuity": {"edges": 5}})), false),
        (edge(json!({"points": "up"})), false),
        (edge(json!({"points": [[1, 2]]})), false),
        (edge(json!({"points": null})), false),
        (json!({"sketch": null}), false),
        (json!({"sketch": true}), false),
        (json!({"sketch": "S", "profiles": null}), false),
        (json!({"sketch": "S", "exact_regions": "yes"}), false),
    ] {
        let issues = crate::schema::issues("surface.patch", &p);
        assert_eq!(issues.is_empty(), takes, "{p}: {issues:?}");
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
}

/// #13434: Surface Patch replies with `PatchResult`, with the keys it gave before, on a loop of
/// edges with each form of continuity, points and a crown, and on a sketch's regions: in Warn,
/// without warnings, so its contract takes each call. Each call makes one surface body and
/// changes no other.
#[test]
fn surface_patch_replies_with_patch_result() {
    let schema = output_schema_for::<PatchResult>();
    let schema = schema.as_value();
    let before =
        BTreeSet::from(["feature", "name", "bodies", "created_bodies", "changed_bodies", "removed_bodies", "warning", "ms", "crown", "continuity"]);
    let first = rim()[0].clone();
    let calls = [
        (on_rim(json!({"continuity": "g1", "body_name": "Cap"})), json!({"all": "g1"})),
        (on_rim(json!({"continuity": {"all": "G1"}, "points": [[0, 0, 38]], "name": "Tall"})), json!({"all": "g1"})),
        (on_rim(json!({"continuity": [{"edge": first, "g": "g2"}], "body_name": "Mixed"})), json!({"edges": [{"edge": first, "g": "g2"}]})),
        (on_rim(json!({"continuity": "g1", "crown": 0.5})), json!({"all": "g1"})),
        (on_rim(json!({"body_names": ["Disc"]})), json!({})),
        (json!({"sketch": "S", "body_name": "Sheet"}), json!({})),
        (json!({"sketch": "S", "profiles": [0], "exact_regions": true, "continuity": "g0", "points": [], "crown": 0}), json!({})),
    ];
    for (p, continuity) in calls {
        let mut s = tube(ParamCheck::Warn);
        let r = s.execute("surface.patch", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        let keys: BTreeSet<&str> = r.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, before, "{p}: {r}");
        assert_eq!(r["continuity"], continuity, "{p}: {r}");
        let count = |key: &str| r[key].as_array().map_or(usize::MAX, Vec::len);
        assert_eq!((count("created_bodies"), count("changed_bodies"), count("removed_bodies")), (1, 0, 0), "{p}: {r}");
    }
}
