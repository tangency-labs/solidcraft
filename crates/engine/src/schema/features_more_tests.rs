use serde_json::{Value, json};

use super::{BoundaryFillParams, CoilParams, RemoveParams, RibParams};
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn session(level: ParamCheck, calls: &[(&str, Value)]) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in calls {
        s.execute(id, p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    }
    s
}

/// An L bracket, `L` (a 40 × 20 × 5 base and a 5 × 20 × 40 wall), and `RibSketch` on its middle
/// plane with one line, `l1`, across the inside corner: a rib or web there fills a triangle.
fn bracket(level: ParamCheck) -> Session {
    session(
        level,
        &[
            ("solid.box", json!({"length": 40, "width": 20, "height": 5, "corner": [0, 0, 0], "body_name": "L"})),
            ("solid.box", json!({"length": 5, "width": 20, "height": 40, "corner": [0, 0, 0], "operation": "join"})),
            ("sketch.create", json!({"plane": {"origin": [0, 10, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "RibSketch"})),
            ("sketch.line", json!({"points": [[5, 25], [25, 5]], "ids": ["l1"]})),
            ("sketch.finish", json!({})),
        ],
    )
}

/// Two 20 mm cubes, `A` and `B`, overlapping where 10 ≤ x ≤ 20.
fn boxes(level: ParamCheck) -> Session {
    session(
        level,
        &[
            ("solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "A"})),
            ("solid.box", json!({"corner": [10, 0, 0], "length": 20, "width": 20, "height": 20, "body_name": "B"})),
        ],
    )
}

/// `Section`, a 1 mm square 10 mm from the Z axis on a plane through it, to coil.
fn coil_section(level: ParamCheck) -> Session {
    session(
        level,
        &[
            ("sketch.create", json!({"plane": "YZ", "name": "Section"})),
            ("sketch.rectangle.two_point", json!({"p0": [10, -0.5], "p1": [11, 0.5]})),
            ("sketch.finish", json!({})),
        ],
    )
}

fn coil() -> Value {
    json!({"diameter": 40, "revolutions": 2, "pitch": 8, "section_size": 4})
}

fn rib() -> Value {
    json!({"sketch": "RibSketch", "curves": ["l1"], "thickness": 4})
}

/// `base` with `key` set to `value`.
fn with(base: &Value, key: &str, value: Value) -> Value {
    let mut p = base.clone();
    p[key] = value;
    p
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<RibParams>(),
        &["body_name", "body_names", "curves", "depth", "direction", "flip", "name", "participants", "sketch", "thickness", "touching"],
    );
    assert_keys(input_schema_for::<RemoveParams>(), &["bodies", "body_name", "body_names", "name", "participants"]);
    assert_keys(
        input_schema_for::<BoundaryFillParams>(),
        &["body_name", "body_names", "cells", "name", "operation", "participants", "remove_tools", "tools", "touching"],
    );
    assert_keys(
        input_schema_for::<CoilParams>(),
        &[
            "axis",
            "base",
            "body_name",
            "body_names",
            "clockwise",
            "diameter",
            "height",
            "name",
            "operation",
            "participants",
            "pitch",
            "profiles",
            "revolutions",
            "section",
            "section_position",
            "section_size",
            "sketch",
            "start_angle",
            "targets",
            "turns",
        ],
    );
}

/// As `assert_strict_keeps_todays_errors`, on the design `fixture` makes: for the calls that need
/// the sketch's curve or the bodies to get past the handler's first checks.
fn assert_strict_keeps_errors_on(fixture: fn(ParamCheck) -> Session, cases: &[(&str, Value)]) {
    for (id, p) in cases {
        let off = fixture(ParamCheck::Off).execute(id, p).err().map(|e| e.to_string());
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(fixture(ParamCheck::Strict).execute(id, p).err().map(|e| e.to_string()), off, "{id} {p}");
    }
}

/// #12655: a `null` or empty `curves`, `bodies`, `tools` or `cells`, which the handlers refuse,
/// is refused by the contracts too, with the handlers' messages. So the check (warn, strict, MCP
/// validation, batch preflight) catches it before the handler runs. Only a key left out goes on
/// to the handler, for its own message.
#[test]
fn the_contracts_refuse_null_and_empty_where_the_handlers_do() {
    let curves = "`curves` must list 1…1000 sketch curve ids";
    let cells = "`cells` must list 1…1000 points [x, y, z], one inside each cell";
    for (id, p, message) in [
        ("solid.rib", json!({"curves": null, "thickness": 1}), curves),
        ("solid.web", json!({"curves": [], "thickness": 1}), curves),
        ("solid.remove", json!({"bodies": null}), "`bodies` must list bodies"),
        ("solid.remove", json!({"bodies": []}), "`bodies` must list bodies"),
        ("solid.boundary_fill", json!({"tools": null, "cells": [[1, 2, 3]]}), "`tools` must list 1…64 bodies"),
        ("solid.boundary_fill", json!({"tools": ["A"], "cells": null}), cells),
        ("solid.boundary_fill", json!({"tools": ["A"], "cells": []}), cells),
    ] {
        let messages: Vec<String> = crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect();
        assert_eq!(messages, [message], "{id} {p}");
    }
    for (id, p) in [("solid.rib", json!({"thickness": 1})), ("solid.remove", json!({})), ("solid.boundary_fill", json!({}))] {
        assert!(crate::schema::issues(id, &p).is_empty(), "{id} {p}: left out, the handler refuses it");
    }
}

/// #12655: calls that fail today get the same message in strict. The first ones fail on the
/// shared helper's sketch; the rest need the bracket's curve or the two boxes.
#[test]
fn strict_keeps_todays_errors() {
    let many_curves = vec!["l1"; 1001];
    let many_tools: Vec<String> = (0..65).map(|i| format!("T{i}")).collect();
    assert_strict_keeps_todays_errors(&[
        ("solid.rib", json!({"curves": null, "thickness": 1})),
        ("solid.rib", json!({"curves": [], "thickness": 1})),
        ("solid.rib", json!({"curves": 5, "thickness": 1})),
        ("solid.rib", json!({"curves": ["l1", 5], "thickness": 1})),
        ("solid.rib", json!({"curves": many_curves, "thickness": 1})),
        ("solid.rib", json!({"curves": ["nope"], "thickness": 1})),
        ("solid.rib", json!({"sketch": true, "curves": ["l1"], "thickness": 1})),
        ("solid.rib", json!({"sketch": "Nope", "curves": ["l1"], "thickness": 1})),
        ("solid.web", json!({"curves": [], "thickness": 1})),
        ("solid.coil", json!({"diameter": 40, "section_size": 4, "revolutions": 2})),
        ("solid.coil", json!({"diameter": 40, "section_size": 4, "revolutions": 2, "pitch": 8, "section": "Hexagon"})),
        ("solid.coil", json!({"diameter": 40, "section_size": 4, "revolutions": 2, "pitch": 8, "section_position": "Middle"})),
        ("solid.coil", json!({"diameter": 40, "section_size": 4, "revolutions": 2, "pitch": 8, "base": null})),
        ("solid.coil", json!({"diameter": 40, "section_size": 4, "revolutions": 2, "pitch": 8, "base": [1, 2]})),
        ("solid.coil", json!({"diameter": 40, "section_size": 4, "revolutions": 2, "pitch": 8, "axis": null})),
        ("solid.coil", json!({"diameter": 40, "section_size": 4, "revolutions": 2, "pitch": 8, "axis": [0, 0, 0]})),
        ("solid.coil", json!({"diameter": 40, "section_size": 4, "revolutions": 2, "pitch": 8, "profiles": [0]})),
        ("solid.coil", json!({"diameter": 40, "section_size": 4, "revolutions": 2, "pitch": 8, "operation": "glue"})),
        ("solid.coil", json!({"sketch": "Nope", "revolutions": 2, "pitch": 8})),
        ("solid.remove", json!({})),
        ("solid.remove", json!({"bodies": null})),
        ("solid.remove", json!({"bodies": []})),
        ("solid.remove", json!({"bodies": 5})),
        ("solid.remove", json!({"bodies": ["Nope"]})),
        ("solid.boundary_fill", json!({})),
        ("solid.boundary_fill", json!({"tools": null, "cells": [[5, 5, 5]]})),
        ("solid.boundary_fill", json!({"tools": [], "cells": [[5, 5, 5]]})),
        ("solid.boundary_fill", json!({"tools": many_tools, "cells": [[5, 5, 5]]})),
        ("solid.boundary_fill", json!({"tools": ["A"], "cells": [[5, 5, 5]]})),
    ]);
    assert_strict_keeps_errors_on(
        bracket,
        &[
            ("solid.rib", with(&rib(), "thickness", json!(null))),
            ("solid.rib", with(&rib(), "thickness", json!("nope"))),
            ("solid.rib", with(&rib(), "depth", json!("2 deg"))),
            ("solid.rib", with(&rib(), "direction", json!("sideways"))),
            ("solid.web", with(&rib(), "direction", json!("SIDEWAYS"))),
        ],
    );
    let many_cells = vec![[5, 5, 5]; 1001];
    assert_strict_keeps_errors_on(
        boxes,
        &[
            ("solid.boundary_fill", json!({"tools": ["A"]})),
            ("solid.boundary_fill", json!({"tools": ["A"], "cells": null})),
            ("solid.boundary_fill", json!({"tools": ["A"], "cells": []})),
            ("solid.boundary_fill", json!({"tools": ["A"], "cells": ["x"]})),
            ("solid.boundary_fill", json!({"tools": ["A"], "cells": many_cells})),
            ("solid.boundary_fill", json!({"tools": ["A"], "cells": [[5, 5, 5]], "operation": "glue"})),
        ],
    );
}

/// #12655: keys of the wrong type that the handlers ignore today (an item of `cells` that isn't a
/// point is skipped; a `flip` or `remove_tools` that isn't true or false is read as false) are
/// refused by the contracts: in Warn the call still works, with a warning naming the key.
#[test]
fn the_contracts_refuse_what_the_handlers_ignore() {
    let fill = json!({"tools": ["A", "B"], "cells": [[15, 10, 10]]});
    let cases: Vec<(fn(ParamCheck) -> Session, &str, Value, &str)> = vec![
        (bracket, "solid.rib", with(&rib(), "flip", json!("yes")), "flip"),
        (bracket, "solid.rib", with(&rib(), "depth", json!(true)), "depth"),
        (bracket, "solid.web", with(&rib(), "direction", json!(5)), "direction"),
        (boxes, "solid.boundary_fill", with(&fill, "cells", json!([[15, 10, 10], "x"])), "cells"),
        (boxes, "solid.boundary_fill", with(&fill, "remove_tools", json!("yes")), "remove_tools"),
        (boxes, "solid.boundary_fill", with(&fill, "operation", json!(5)), "operation"),
    ];
    for (fixture, id, p, key) in cases {
        assert!(fixture(ParamCheck::Off).execute(id, &p).is_ok(), "{id} {p} works today");
        let warned = fixture(ParamCheck::Warn).execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(warned["param_warnings"].to_string().contains(key), "{warned}");
        let error = fixture(ParamCheck::Strict).execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains(&format!("`{key}`")), "{id} {p}: {error}");
    }
}

/// #12655: the schemas take what the handlers take. Each call is checked against both the schema
/// and the handler, with the check off, so the two can't drift apart.
#[test]
fn the_schemas_take_what_the_handlers_take() {
    let agree = |fixture: fn(ParamCheck) -> Session, id: &str, schema: schemars::Schema, calls: &[Value]| {
        let schema = schema.as_value();
        for p in calls {
            let takes = fixture(ParamCheck::Off).execute(id, p).is_ok();
            assert_eq!(conforms(schema, schema, p), takes, "{id} {p}");
        }
    };
    agree(
        bracket,
        "solid.rib",
        input_schema_for::<RibParams>(),
        &[
            rib(),
            with(&rib(), "curves", json!("l1")),
            with(&rib(), "thickness", json!("4 mm")),
            with(&rib(), "depth", json!(2)),
            with(&with(&rib(), "flip", json!(false)), "direction", json!("in_plane")),
            with(&rib(), "curves", json!([])),
            with(&rib(), "thickness", json!("")),
            with(&rib(), "sketch", json!([1])),
        ],
    );
    agree(
        boxes,
        "solid.boundary_fill",
        input_schema_for::<BoundaryFillParams>(),
        &[
            json!({"tools": ["A", "B"], "cells": [[15, 10, 10]]}),
            json!({"tools": ["A", "B"], "cells": [{"x": 5, "y": 10, "z": 10}, [25, 10, 10]], "remove_tools": true}),
            json!({"tools": "A", "cells": [[5, 10, 10]], "operation": "new", "body_name": "Fill"}),
            json!({"tools": [], "cells": [[15, 10, 10]]}),
            json!({"tools": ["A", "B"], "cells": []}),
            json!({"tools": ["A", "B"], "cells": [[15, 10]]}),
            json!({"tools": ["A", "B"], "cells": [[15, 10, 10]], "operation": "glue"}),
        ],
    );
    agree(
        boxes,
        "solid.remove",
        input_schema_for::<RemoveParams>(),
        &[json!({"bodies": "B"}), json!({"bodies": ["A", "B"], "name": "Gone"}), json!({"bodies": []}), json!({"bodies": [5]}), json!({})],
    );
    agree(
        coil_section,
        "solid.coil",
        input_schema_for::<CoilParams>(),
        &[coil(), with(&coil(), "section", json!("hexagon")), with(&coil(), "section_position", json!("middle"))],
    );
}

/// #12655: each command replies as a feature command does, in Warn without warnings: a rib and a
/// web on the bracket, the two boxes' shared lens filled, and a box removed. A misspelt key is
/// ignored today: Warn says so, and strict refuses it.
#[test]
fn the_features_reply_with_their_schemas() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let check = |s: &mut Session, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let r = check(&mut bracket(ParamCheck::Warn), "solid.rib", with(&rib(), "name", json!("Brace")));
    assert_eq!((&r["name"], &r["changed_bodies"]), (&json!("Brace"), &json!(["L"])), "{r}");
    check(&mut bracket(ParamCheck::Warn), "solid.web", rib());
    let mut s = boxes(ParamCheck::Warn);
    let r = check(&mut s, "solid.boundary_fill", json!({"tools": ["A", "B"], "cells": [[15, 10, 10]], "body_name": "Lens"}));
    assert_eq!(r["created_bodies"], json!(["Lens"]), "{r}");
    let r = check(&mut s, "solid.remove", json!({"bodies": "Lens"}));
    assert_eq!(r["removed_bodies"], json!(["Lens"]), "{r}");
    // Every form Coil takes: the three ways to give its length, each option, and a sketch section.
    for p in [
        coil(),
        json!({"diameter": 40, "turns": 2, "height": 16, "section_size": 4, "body_name": "Spring"}),
        json!({"diameter": "40 mm", "height": 16, "pitch": "8 mm", "section_size": 4, "section": "Square", "section_position": "outside"}),
        json!({"diameter": 40, "revolutions": 2, "pitch": 8, "section_size": 4, "section": "circle", "section_position": "on_center", "base": [50, "2 mm", 0], "axis": "Y", "start_angle": "30 deg", "clockwise": true}),
        json!({"diameter": 40, "revolutions": 2, "pitch": 8, "section_size": 4, "axis": [0, 0, 1], "section_position": "inside", "operation": "new", "name": "Helix"}),
    ] {
        check(&mut Session { param_check: ParamCheck::Warn, ..Session::default() }, "solid.coil", p);
    }
    for p in [
        json!({"sketch": "Section", "revolutions": 3, "pitch": 8}),
        json!({"sketch": "Section", "profiles": [0], "revolutions": 3, "pitch": 8, "clockwise": true}),
    ] {
        check(&mut coil_section(ParamCheck::Warn), "solid.coil", p);
    }
    let mut s = boxes(ParamCheck::Warn);
    let r = check(
        &mut s,
        "solid.coil",
        json!({"diameter": 12, "revolutions": 2, "pitch": 6, "section_size": 3, "base": [10, 10, 4], "operation": "join", "targets": ["A"]}),
    );
    assert_eq!(r["changed_bodies"], json!(["A"]), "{r}");
    // A misspelt key.
    let p = with(&rib(), "flp", json!(true));
    let r = bracket(ParamCheck::Warn).execute("solid.rib", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(r["param_warnings"].to_string().contains("flp"), "{r}");
    let error = bracket(ParamCheck::Strict).execute("solid.rib", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`flp`"), "{error}");
}
