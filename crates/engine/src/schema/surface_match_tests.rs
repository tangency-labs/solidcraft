use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use super::MatchSurfaceParams;
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// Two points on the cylinder's straight vertical wall seam, as the command's own tests pick it.
fn picks(s: &Session) -> Vec<Vec3> {
    let boss = s.world_state().body("Boss").map(|b| b.body.clone()).unwrap_or_else(|| panic!("no Boss"));
    let e = boss
        .edges(boss.size() * 1e-2)
        .unwrap_or_default()
        .into_iter()
        .find(|e| e.length > 25.0 && (e.mid.x.hypot(e.mid.y) - 10.0).abs() < 0.05 && e.mid.z > 5.0 && e.mid.z < 25.0)
        .unwrap_or_else(|| panic!("the cylinder has a straight vertical wall seam"));
    let (a, b) = (e.points.first().copied().unwrap_or(e.mid), e.points.last().copied().unwrap_or(e.mid));
    vec![a + (b - a) * 0.25, a + (b - a) * 0.75]
}

/// A cylinder `Boss` of radius 10, 30 high, and a cylindrical surface `Skirt` of radius 11 from
/// the cylinder's wall seam round 60 degrees, as in the command's own tests: a point on the skirt.
fn boss_and_skirt(s: &mut Session) -> [f64; 3] {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Round"}));
    run(s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"sketch": "Round", "distance": 30, "body_name": "Boss"}));
    let seam = picks(s)[0];
    let a = seam.y.atan2(seam.x);
    let at = |t: f64| json!([11.0 * t.cos(), 11.0 * t.sin()]);
    run(s, "sketch.create", json!({"plane": "XY", "name": "Skirt sketch"}));
    let arc = run(s, "sketch.arc.three_point", json!({"start": at(a), "end": at(a + 60f64.to_radians()), "through": at(a + 30f64.to_radians())}))
        ["created"]["curves"][0]
        .clone();
    run(s, "sketch.finish", json!({}));
    run(s, "surface.extrude", json!({"sketch": "Skirt sketch", "curves": [arc], "distance": 30, "body_name": "Skirt"}));
    let mid = a + 30f64.to_radians();
    [11.0 * mid.cos(), 11.0 * mid.sin(), 15.0]
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<MatchSurfaceParams>(), &["body", "edges", "faces", "name", "order", "participants", "target"]);
}

/// A call that fails today gets the same message in strict. The handler looks both bodies up
/// before it reads the picks, so each call runs on a design with two (a missing `edges` is left
/// out: strict says it is required, the handler what it must list).
#[test]
fn strict_keeps_todays_errors() {
    let many = vec![[0, 0, 0]; 10_001];
    let faces = vec![[0, 0, 0]; 65];
    let e = json!([[0, 0, 0]]);
    for p in [
        json!({"target": "T", "edges": e}),
        json!({"body": 5, "target": "T", "edges": e}),
        json!({"body": "Nope", "target": "T", "edges": e}),
        json!({"body": "B", "edges": e}),
        json!({"body": "B", "target": [], "edges": e}),
        json!({"body": "B", "target": "Nope", "edges": e}),
        json!({"body": "B", "target": "T", "faces": faces, "edges": e}),
        json!({"body": "B", "target": "T", "edges": []}),
        json!({"body": "B", "target": "T", "edges": null}),
        json!({"body": "B", "target": "T", "edges": 5}),
        json!({"body": "B", "target": "T", "edges": [[1, 2]]}),
        json!({"body": "B", "target": "T", "edges": many}),
        json!({"body": "B", "target": "T", "edges": e, "order": "g5"}),
        json!({"body": "B", "target": "T", "edges": e, "order": "g0"}),
        json!({"body": "B", "target": "T", "edges": e, "order": null}),
        json!({"body": "B", "target": "T", "edges": e, "order": 2}),
        json!({"body": "B", "target": "T", "edges": e}),
    ] {
        let error = |level: ParamCheck| {
            let mut s = Session::default();
            run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "body_name": "B"}));
            run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [10, 0, 0], "body_name": "T"}));
            s.param_check = level;
            s.execute("surface.match", &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// #10981: a match replies with `FeatureResult`, in Warn without warnings for good calls (the
/// body's first face, and a picked face with every other key) and with them for a misspelt key;
/// strict refuses a face point the handler would drop.
#[test]
fn a_match_replies_with_the_feature_result() {
    let schema = output_schema_for::<FeatureResult>();
    let matches = |r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let on_skirt = boss_and_skirt(&mut s);
    let chain = picks(&s);
    let r = run(&mut s, "surface.match", json!({"body": "Skirt", "target": "Boss", "edges": chain, "order": "G2"}));
    matches(&r, false);
    assert_eq!(r["changed_bodies"], json!(["Skirt"]), "{r}");
    let r = run(
        &mut s,
        "surface.match",
        json!({"body": "Skirt", "faces": [on_skirt], "target": "Boss", "edges": chain, "order": "g1", "name": "Seam", "participants": ["*"]}),
    );
    matches(&r, false);
    assert_eq!(r["name"], "Seam", "{r}");
    matches(&run(&mut s, "surface.match", json!({"body": "Skirt", "target": "Boss", "edges": chain, "ordr": "g3"})), true);
    s.param_check = ParamCheck::Strict;
    let error = s.execute("surface.match", &json!({"body": "Skirt", "faces": [on_skirt, [1, 2]], "target": "Boss", "edges": chain}));
    let error = error.err().map(|e| e.to_string());
    assert!(error.as_deref().is_some_and(|e| e.contains("`faces` must be a list of points")), "{error:?}");
}
