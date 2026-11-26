//! `surface.morph`: deform a solid or sheet body (or selected faces of it) by mapping a base
//! reference surface onto a target reference surface.

use serde_json::Value;
use solidcraft_doc::{BodyName, Expr, FacePick, FeatureKind, MorphMapMode, MorphProjection, Position};

use super::CommandSpec;
use super::features::add_feature;
use crate::params::{bad, expr, str_, vec3};
use crate::schema::features::FeatureResult;
use crate::schema::surface_morph::MorphSurfaceParams;
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CONTRACT: TypedContract<MorphSurfaceParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("surface.morph", "Morph", crate::checked!("surface.morph", morph_surface))
        .begins_edit()
        .at("SURFACE", "MODIFY")
        .icon("surface_morph")
        .contract(&CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params(
            "body: solid or surface body name; faces?: [[x,y,z]] (optional points on specific faces, default: whole body); base: body name carrying the base reference surface; base_face?: [x,y,z]; control: body name carrying the target reference surface; control_face?: [x,y,z]; projection?: normal|direction (default normal); direction?: [dx,dy,dz]; map_mode?: normal_to_normal|translate (default normal_to_normal); scale?: number or expr (default 1); tolerance?: length or expr (default 0.01 mm); flip_u?: bool; flip_v?: bool; swap_uv?: bool; keep_original?: bool",
        ),
];

fn morph_surface(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.morph";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let base = str_(p, "base").ok_or_else(|| bad(cmd, "`base` is required"))?.to_string();
    if s.model.state().body(&base).is_none() {
        return Err(bad(cmd, format!("no body `{base}`")));
    }
    let control = str_(p, "control").ok_or_else(|| bad(cmd, "`control` is required"))?.to_string();
    if s.model.state().body(&control).is_none() {
        return Err(bad(cmd, format!("no body `{control}`")));
    }

    let faces: Vec<solidcore_record::Vec3> = match p.get("faces") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => {
            a.iter().map(|v| vec3(v).ok_or_else(|| bad(cmd, "`faces` entries must be [x, y, z] points"))).collect::<Result<_>>()?
        }
        Some(_) => return Err(bad(cmd, "`faces` must be an array of [x, y, z] points")),
    };
    if faces.len() > 1024 {
        return Err(bad(cmd, "too many face picks"));
    }

    let base_face = match p.get("base_face") {
        None | Some(Value::Null) => None,
        Some(v) => Some(Position(vec3(v).ok_or_else(|| bad(cmd, "`base_face` must be [x, y, z]"))?)),
    };
    let control_face = match p.get("control_face") {
        None | Some(Value::Null) => None,
        Some(v) => Some(Position(vec3(v).ok_or_else(|| bad(cmd, "`control_face` must be [x, y, z]"))?)),
    };

    let projection = match p.get("projection").and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() {
        None | Some("normal") => MorphProjection::Normal,
        Some("direction") => {
            let d = p
                .get("direction")
                .and_then(vec3)
                .ok_or_else(|| bad(cmd, "`direction` [dx, dy, dz] is required when `projection` is \"direction\""))?;
            if !solidcraft_kernel::services::morph::is_valid_direction([d.x, d.y, d.z]) {
                return Err(bad(cmd, "`direction` must be a non-zero finite vector"));
            }
            MorphProjection::Direction { dir: [d.x, d.y, d.z] }
        }
        Some(other) => return Err(bad(cmd, format!("unknown `projection` `{other}` (expected `normal` or `direction`)"))),
    };

    let map_mode = match p.get("map_mode").and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() {
        None | Some("normal_to_normal") => MorphMapMode::NormalToNormal,
        Some("translate") => MorphMapMode::Translate,
        Some(other) => return Err(bad(cmd, format!("unknown `map_mode` `{other}` (expected `normal_to_normal` or `translate`)"))),
    };

    let scale = expr(p, "scale").unwrap_or_else(|| Expr::from("1"));
    let tolerance = expr(p, "tolerance").unwrap_or_else(|| Expr::from("0.01 mm"));
    let flip_u = p.get("flip_u").and_then(Value::as_bool).unwrap_or(false);
    let flip_v = p.get("flip_v").and_then(Value::as_bool).unwrap_or(false);
    let swap_uv = p.get("swap_uv").and_then(Value::as_bool).unwrap_or(false);
    let keep_original = p.get("keep_original").and_then(Value::as_bool).unwrap_or(false);

    add_feature(
        s,
        p,
        FeatureKind::SurfaceMorph {
            body: BodyName::from(body),
            faces: faces.into_iter().map(FacePick).collect(),
            base: BodyName::from(base),
            base_face,
            control: BodyName::from(control),
            control_face,
            projection,
            map_mode,
            scale,
            tolerance,
            flip_u,
            flip_v,
            swap_uv,
            keep_original,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;
    use serde_json::json;

    fn run(s: &mut Session, c: &str, p: Value) -> Value {
        s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"))
    }

    #[test]
    fn surface_morph_deforms_solid_box_and_supports_undo() {
        let mut s = Session::new(solidcraft_doc::Document::new("test"));
        // Target solid box: 10 x 10 x 2
        run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": 10, "width": 10, "height": 2, "body_name": "Block"}));

        // Flat base sheet on Z = 0
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "BaseSk"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.patch", json!({"sketch": "BaseSk", "body_name": "BaseSrf"}));

        // Raised target sheet on Z = 5
        run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 5, "name": "TopPl"}));
        run(&mut s, "sketch.create", json!({"plane": "TopPl", "name": "CtrlSk"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.patch", json!({"sketch": "CtrlSk", "body_name": "CtrlSrf"}));

        let vol_before = solidcraft_kernel::measure(&s.model.state().body("Block").unwrap().body).unwrap().volume;
        run(
            &mut s,
            "surface.morph",
            json!({"body": "Block", "base": "BaseSrf", "control": "CtrlSrf", "scale": 1.0, "tolerance": 0.05, "name": "Morph1"}),
        );
        let morphed = s.model.state().body("Block").unwrap().body.clone();
        let m = solidcraft_kernel::measure(&morphed).unwrap();
        assert!((m.volume - vol_before).abs() < 1.0, "volume drifted: {} vs {}", m.volume, vol_before);
        assert!(m.bbox.min.z > 4.5 && m.bbox.max.z > 6.5, "expected block raised by +5 mm: {:?}", m.bbox);

        // Editing / redefining the Morph feature in place updates the body (scale 0.5 -> +2.5 mm)
        run(
            &mut s,
            "timeline.redefine",
            json!({
                "feature": "Morph1",
                "command": "surface.morph",
                "params": {"body": "Block", "base": "BaseSrf", "control": "CtrlSrf", "scale": 0.5, "tolerance": 0.05}
            }),
        );
        let edited = solidcraft_kernel::measure(&s.model.state().body("Block").unwrap().body).unwrap();
        assert!(edited.bbox.min.z > 2.0 && edited.bbox.min.z < 3.0, "expected block raised by +2.5 mm: {:?}", edited.bbox);

        // Undo redefine, then undo morph restores original position
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "edit.undo", json!({}));
        let restored = solidcraft_kernel::measure(&s.model.state().body("Block").unwrap().body).unwrap();
        assert!(restored.bbox.min.z.abs() < 0.1);
    }

    #[test]
    fn surface_morph_rejects_bad_params() {
        let mut s = Session::new(solidcraft_doc::Document::new("test"));
        run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": 10, "width": 10, "height": 2, "body_name": "Block"}));
        assert!(s.execute("surface.morph", &json!({"body": "Nope", "base": "Block", "control": "Block"})).is_err());
        assert!(s.execute("surface.morph", &json!({"body": "Block", "base": "Nope", "control": "Block"})).is_err());
        assert!(s.execute("surface.morph", &json!({"body": "Block", "base": "Block", "control": "Nope"})).is_err());
        assert!(s.execute("surface.morph", &json!({"body": "Block", "base": "Block", "control": "Block", "projection": "direction"})).is_err());
        assert!(s.execute("surface.morph", &json!({"body": "Block", "base": "Block", "control": "Block", "projection": "bogus"})).is_err());
        assert!(s.execute("surface.morph", &json!({"body": "Block", "base": "Block", "control": "Block", "map_mode": "bogus"})).is_err());
        // Malformed picks must be rejected instead of silently dropped to whole-body / face 0.
        assert!(s.execute("surface.morph", &json!({"body": "Block", "base": "Block", "control": "Block", "faces": "not-an-array"})).is_err());
        assert!(s.execute("surface.morph", &json!({"body": "Block", "base": "Block", "control": "Block", "faces": [[1, 2]]})).is_err());
        assert!(s.execute("surface.morph", &json!({"body": "Block", "base": "Block", "control": "Block", "base_face": [1, 2]})).is_err());
        assert!(s.execute("surface.morph", &json!({"body": "Block", "base": "Block", "control": "Block", "control_face": "bad"})).is_err());
        // Zero direction rejected; tiny finite direction accepted by direction parser.
        assert!(
            s.execute(
                "surface.morph",
                &json!({"body": "Block", "base": "Block", "control": "Block", "projection": "direction", "direction": [0, 0, 0]})
            )
            .is_err()
        );
    }

    #[test]
    fn surface_morph_respects_body_offsets_and_keep_original_placement() {
        let mut s = Session::new(solidcraft_doc::Document::new("test"));
        run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": 10, "width": 10, "height": 2, "body_name": "Block"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "BaseSk"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.patch", json!({"sketch": "BaseSk", "body_name": "BaseSrf"}));
        run(&mut s, "surface.patch", json!({"sketch": "BaseSk", "body_name": "CtrlSrf"}));

        // Offset Block, BaseSrf, and CtrlSrf via document body_offsets (world placement).
        let mut block_off = solidcraft_doc::IDENTITY;
        block_off[3][0] = 100.0;
        let mut ctrl_world = block_off;
        ctrl_world[3][2] = 6.0;
        let doc_mut = std::sync::Arc::make_mut(&mut s.doc);
        doc_mut.body_offsets.insert("Block".into(), block_off);
        doc_mut.body_offsets.insert("BaseSrf".into(), block_off);
        doc_mut.body_offsets.insert("CtrlSrf".into(), ctrl_world);
        s.model.evaluate(&s.doc);

        run(
            &mut s,
            "surface.morph",
            json!({"body": "Block", "base": "BaseSrf", "control": "CtrlSrf", "scale": 1.0, "tolerance": 0.05, "keep_original": true, "name": "MorphOffset"}),
        );
        // The newly created copy must sit at X ∈ [100, 110] and Z ∈ [6, 8].
        let st = s.model.state();
        let copy = st.bodies.iter().find(|b| b.name != "Block" && b.name != "BaseSrf" && b.name != "CtrlSrf").unwrap();
        let m = solidcraft_kernel::measure(&copy.body).unwrap();
        assert!(m.bbox.min.x > 99.5 && m.bbox.max.x < 110.5, "Keep Original lost X offset: {:?}", m.bbox);
        assert!(m.bbox.min.z > 5.5 && m.bbox.max.z < 8.5, "Morph ignored CtrlSrf Z offset: {:?}", m.bbox);
    }
}
