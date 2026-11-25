use serde_json::{Value, json};

use super::{
    MaterialCreateParams, MaterialDeleteParams, MaterialDeleted, MaterialEditParams, MaterialInspectParams, MaterialInspected, MaterialReply,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// An original synthetic physical card, as `cmd/material_card_persistence_tests.rs` uses: a
/// density and an isotropic elasticity.
fn card() -> Value {
    json!({"schema_version":1,"designation":"Original synthetic card, not design data","family":"other",
        "sources":[{"id":"fixture","title":"Original analytic constants","kind":"illustrative","license":"MIT OR Apache-2.0"}],
        "properties":[{"source_id":"fixture","conditions":{"temperature_k":293.15,"state":"synthetic"},
            "data":{"kind":"density","kg_m3":700.0}},
            {"source_id":"fixture","data":{"kind":"elasticity","model":{
                "symmetry":"isotropic","young_modulus_pa":12.25e9,"poisson_ratio":0.25}}}]})
}

/// A design with a box, `Specimen`, and a material of its own, `Timber`, assigned to it, at
/// the parameter-check level `level`.
fn with_timber(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in [
        ("solid.box", json!({"body_name": "Specimen", "length": 10, "width": 20, "height": 5})),
        ("material.create", json!({"name": "Timber", "density_g_cm3": 0.7, "color": "#b47846"})),
        ("material.assign", json!({"bodies": ["Specimen"], "material": "Timber"})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    }
    s
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<MaterialCreateParams>(), &["appearance", "color", "density_g_cm3", "name", "opacity", "pbr", "physical_card"]);
    assert_keys(
        input_schema_for::<MaterialEditParams>(),
        &["appearance", "color", "density_g_cm3", "material", "name", "opacity", "pbr", "physical_card"],
    );
    assert_keys(input_schema_for::<MaterialInspectParams>(), &["material"]);
    assert_keys(input_schema_for::<MaterialDeleteParams>(), &["material", "replace"]);
}

/// Calls that fail today get the same message in strict: the contract's readers give the
/// handler's own message (`density_g_cm3`, `opacity`, `color`, `material`), and the handler
/// refuses the rest.
#[test]
fn strict_keeps_todays_errors() {
    let new = |extra: Value| {
        let mut p = json!({"name": "Resin", "density_g_cm3": 1.2});
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        p
    };
    assert_strict_keeps_todays_errors(&[
        ("material.create", new(json!({"name": " "}))),
        ("material.create", new(json!({"name": "Steel"}))),
        ("material.create", new(json!({"density_g_cm3": 0}))),
        ("material.create", new(json!({"density_g_cm3": "light"}))),
        ("material.create", new(json!({"density_g_cm3": null}))),
        ("material.create", new(json!({"color": "#zzzzzz"}))),
        ("material.create", new(json!({"color": "#808080", "opacity": 2}))),
        ("material.create", new(json!({"color": "#808080", "opacity": null}))),
        ("material.create", new(json!({"opacity": 0.5}))),
        ("material.create", new(json!({"appearance": "Nope"}))),
        ("material.create", new(json!({"appearance": "Steel", "color": "#808080"}))),
        ("material.create", new(json!({"pbr": {"metallic": 0, "roughness": 0.4, "dielectric_ior": 1.5}}))),
        ("material.create", new(json!({"color": "#808080", "pbr": {"metallic": 2, "roughness": 0.4, "dielectric_ior": 1.5}}))),
        ("material.create", new(json!({"physical_card": {}}))),
        ("material.inspect", json!({"material": "Unobtanium"})),
        ("material.inspect", json!({"material": 5})),
    ]);
    // Edit and delete need a material of the design's own, and one in use.
    let cases = [
        ("material.edit", json!({"material": "Nope"})),
        ("material.edit", json!({"material": 5})),
        ("material.edit", json!({"material": "Timber", "density_g_cm3": null})),
        ("material.edit", json!({"material": "Timber", "name": "Aluminum"})),
        ("material.edit", json!({"material": "Timber", "opacity": 0.5})),
        ("material.delete", json!({"material": "Timber"})),
        ("material.delete", json!({"material": "Timber", "replace": "timber"})),
        ("material.delete", json!({"material": "Timber", "replace": "Nope"})),
    ];
    for (id, p) in &cases {
        let error = |level: ParamCheck| with_timber(level).execute(id, p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

/// #11862: creating, inspecting, editing and deleting materials reply as the output schemas say,
/// in Warn without warnings: with a card (its check and density comparison) and without, a
/// built-in material, a look cleared with `color: null`, and a deletion that moves the bodies to
/// a built-in material. A misspelt key gets `param_warnings` in Warn, since the handlers ignore
/// unknown keys, and is refused in Strict.
#[test]
fn material_commands_reply_with_their_schemas() {
    let (material, inspected, deleted) =
        (output_schema_for::<MaterialReply>(), output_schema_for::<MaterialInspected>(), output_schema_for::<MaterialDeleted>());
    let mut s = with_timber(ParamCheck::Warn);
    let run = |s: &mut Session, schema: &schemars::Schema, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let p = json!({"name": "Resin", "density_g_cm3": 1.2, "color": [40, 90, 200], "opacity": 1,
        "pbr": {"metallic": 0, "roughness": 0.4, "dielectric_ior": 1.5}, "physical_card": card()});
    let r = run(&mut s, &material, "material.create", p);
    assert!(r["physical_validation"].is_object() && r["density_comparison"].is_object() && r.get("bodies").is_none(), "{r}");
    run(&mut s, &material, "material.create", json!({"name": "Plaster", "density_g_cm3": 0.9, "appearance": "Steel - Satin"}));
    assert!(run(&mut s, &inspected, "material.inspect", json!({"material": "resin"}))["physical_card"].is_object());
    let r = run(&mut s, &inspected, "material.inspect", json!({"material": "Steel"}));
    assert!(r["user"] == false && r["physical_card"].is_null(), "a built-in material: {r}");
    let p = json!({"material": "Timber", "name": "Larch", "density_g_cm3": 0.75, "color": null, "physical_card": null});
    let r = run(&mut s, &material, "material.edit", p);
    assert!(r["appearance"].is_null() && r["bodies"] == json!(["Specimen"]), "{r}");
    let r = run(&mut s, &deleted, "material.delete", json!({"material": "Larch", "replace": "Steel"}));
    assert!(r["replaced_by"] == "Steel" && r["bodies"] == json!(["Specimen"]), "{r}");
    let r = run(&mut s, &deleted, "material.delete", json!({"material": "Plaster"}));
    assert!(r["replaced_by"].is_null(), "{r}");
    let r = s.execute("material.inspect", &json!({"material": "Resin", "materal": "Resin"})).unwrap_or_else(|e| panic!("{e}"));
    let warnings = r["param_warnings"].to_string();
    assert!(warnings.contains("materal"), "{r}");
    let mut strict = with_timber(ParamCheck::Strict);
    let error = strict.execute("material.inspect", &json!({"material": "Timber", "materal": "Timber"})).err().map(|e| e.to_string());
    assert!(error.is_some_and(|e| e.contains("`materal`")), "strict refuses the misspelt key");
}
