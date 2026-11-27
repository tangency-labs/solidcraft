use serde_json::json;

use super::{DogboneParams, DogboneResult};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<DogboneParams>(),
        &["along", "body", "body_name", "body_names", "diameter", "edges", "faces", "gap", "max_angle", "name", "participants", "side", "style"],
    );
}

/// A 40 x 30 x 10 plate `Plate` with a 20 x 14 pocket 6 deep from its top, at `level`: floor at
/// z = 4, corner edges at x 10 and 30, y 8 and 22.
fn pocketed(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("solid.box", &json!({"length": 20, "width": 14, "height": 7, "corner": [10, 8, 4], "operation": "cut"}))
        .unwrap_or_else(|e| panic!("{e}"));
    s
}

/// A point on the pocket's corner edge at (10, 8).
const CORNER: [f64; 3] = [10.0, 8.0, 7.0];

/// Calls that fail today give the same message in strict, each on a fresh [`pocketed`] plate:
/// what the contract checks (the picks' form, the diameter's type, the style and side, the axis)
/// and what it leaves to the handler (no picks, a diameter, gap or angle out of range or naming
/// no parameter, a gap or side for another style, a body the design lacks, and picks with
/// nothing to relieve). Left out: a call without `diameter` (strict says "`diameter` is
/// required", as for every missing key), and calls that work today but that strict refuses (an
/// unknown key, a `body` or `gap` of the wrong type, which the handler reads as not given).
#[test]
fn strict_keeps_todays_errors() {
    let cases = [
        json!({"diameter": 3}),
        json!({"edges": null, "faces": null, "diameter": 3}),
        json!({"edges": [], "diameter": 3}),
        json!({"edges": 5, "diameter": 3}),
        json!({"edges": [[1, 2]], "diameter": 3}),
        json!({"edges": [true], "diameter": 3}),
        json!({"faces": [], "diameter": 3}),
        json!({"faces": 5, "diameter": 3}),
        json!({"faces": [[20, 15]], "diameter": 3}),
        json!({"edges": [CORNER], "diameter": true}),
        json!({"edges": [CORNER], "diameter": "  "}),
        json!({"edges": [CORNER], "diameter": "no_such_parameter"}),
        json!({"edges": [CORNER], "diameter": 0}),
        json!({"edges": [CORNER], "diameter": -3}),
        json!({"edges": [CORNER], "diameter": "30 deg"}),
        json!({"edges": [CORNER], "diameter": 3, "style": "bogus"}),
        json!({"edges": [CORNER], "diameter": 3, "gap": 0.1}),
        json!({"edges": [CORNER], "diameter": 3, "style": "minimal", "gap": -1}),
        json!({"edges": [CORNER], "diameter": 3, "side": "shorter"}),
        json!({"edges": [CORNER], "diameter": 3, "style": "t_bone", "side": "middle"}),
        json!({"edges": [CORNER], "diameter": 3, "max_angle": "270 deg"}),
        json!({"edges": [CORNER], "diameter": 3, "max_angle": 0}),
        json!({"edges": [CORNER], "diameter": 3, "max_angle": "5 mm"}),
        json!({"edges": [CORNER], "diameter": 3, "along": [0, 0, 0]}),
        json!({"edges": [CORNER], "diameter": 3, "along": "W"}),
        json!({"edges": [CORNER], "diameter": 3, "along": true}),
        json!({"edges": [CORNER], "diameter": 3, "body": "Nope"}),
        json!({"edges": [[0, 0, 5]], "diameter": 3}),
        json!({"edges": [[20, 8, 4]], "diameter": 3, "along": "Z"}),
    ];
    for p in cases {
        let error = |level: ParamCheck| pocketed(level).execute("solid.dogbone", &p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// The contract takes what the input schema allows: each call's check finds no issue exactly
/// when the call matches the schema. A call without picks, or a gap for the normal style,
/// matches it; the handler refuses them.
#[test]
fn the_check_takes_what_the_schema_allows() {
    let schema = input_schema_for::<DogboneParams>();
    let schema = schema.as_value();
    for (p, takes) in [
        (json!({"edges": [[0, 0, 0]], "diameter": 3}), true),
        (json!({"faces": [[0, 0, 0]], "diameter": "3 mm"}), true),
        (json!({"edges": [{"body": "B", "index": 0}], "faces": ["F1"], "diameter": "d", "body": "B", "name": "Bones"}), true),
        (json!({"edges": [[0, 0, 0]], "diameter": 3, "style": "minimal", "gap": "0.1 mm"}), true),
        (json!({"edges": [[0, 0, 0]], "diameter": 3, "style": "t_bone", "side": "shorter"}), true),
        (json!({"edges": [[0, 0, 0]], "diameter": 3, "max_angle": "120 deg", "along": "Z"}), true),
        (json!({"edges": [[0, 0, 0]], "diameter": 3, "along": [0, 0, 1]}), true),
        (json!({"edges": null, "faces": [[0, 0, 0]], "diameter": 3, "style": "tbone", "side": "long"}), true),
        (
            json!({"edges": [[0, 0, 0]], "diameter": 3, "style": null, "gap": null, "side": null, "max_angle": null, "along": null, "body": null}),
            true,
        ),
        (json!({"diameter": 3}), true),
        (json!({"edges": [[0, 0, 0]], "diameter": 3, "gap": 1}), true),
        (json!({"edges": [[0, 0, 0]]}), false),
        (json!({"edges": [], "diameter": 3}), false),
        (json!({"edges": [[0, 0]], "diameter": 3}), false),
        (json!({"faces": 5, "diameter": 3}), false),
        (json!({"edges": [[0, 0, 0]], "diameter": true}), false),
        (json!({"edges": [[0, 0, 0]], "diameter": 3, "style": "bogus"}), false),
        (json!({"edges": [[0, 0, 0]], "diameter": 3, "style": 5}), false),
        (json!({"edges": [[0, 0, 0]], "diameter": 3, "side": "middle"}), false),
        (json!({"edges": [[0, 0, 0]], "diameter": 3, "along": true}), false),
        (json!({"edges": [[0, 0, 0]], "diameter": 3, "along": [0, 0]}), false),
        (json!({"edges": [[0, 0, 0]], "diameter": 3, "body": 5}), false),
        (json!({"edges": [[0, 0, 0]], "diameter": 3, "colour": "red"}), false),
    ] {
        let issues = crate::schema::issues("solid.dogbone", &p);
        assert_eq!(issues.is_empty(), takes, "{p}: {issues:?}");
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
}

/// `solid.dogbone` replies with `DogboneResult` in each of its forms, its contract taking each:
/// edges, a face, each style with its options, an angle and an axis, a pick it skips, and every
/// optional key `null`. In Warn, without warnings.
#[test]
fn dogbone_replies_match_its_output_schema() {
    let schema = output_schema_for::<DogboneResult>();
    let schema = schema.as_value();
    let corners = json!([[10, 8, 7], [30, 8, 7], [30, 22, 7], [10, 22, 7]]);
    let calls = [
        json!({"edges": corners, "diameter": 3, "name": "Bones", "body": "Plate"}),
        json!({"faces": [[20, 15, 4]], "diameter": "3 mm"}),
        json!({"faces": [[20, 15, 4]], "diameter": 3, "style": "minimal", "gap": "0.2 mm"}),
        json!({"faces": [[20, 15, 4]], "diameter": 3, "style": "Minimal"}),
        json!({"edges": corners, "diameter": 3, "style": "t_bone", "side": "shorter"}),
        json!({"edges": corners, "diameter": 3, "style": "TBone", "side": "Long"}),
        json!({"edges": [CORNER], "diameter": 3, "max_angle": "120 deg", "along": "Z"}),
        json!({"edges": [CORNER, [0, 0, 5]], "diameter": 3}),
        json!({"edges": [CORNER], "faces": null, "diameter": 3, "style": null, "gap": null, "side": null, "max_angle": null, "along": null, "body": null}),
    ];
    for p in calls {
        let mut s = pocketed(ParamCheck::Warn);
        let r = s.execute("solid.dogbone", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        assert!(r["relieved"].as_u64().is_some_and(|n| n >= 1), "{p}: {r}");
        assert_eq!(r["changed_bodies"], json!(["Plate"]), "{p}: {r}");
    }
}
