//! `surface.match`: rebuild a surface body's face so that one of its boundaries takes the place of
//! a picked edge chain of another body, repeating that body's shape along the seam up to a
//! requested continuity, while the boundary opposite it keeps its place. The body keeps its name:
//! a match is a modify, like Trim, Extend and Fair.

use serde_json::Value;
use solidcraft_doc::{BodyName, FacePick, FeatureKind, Position};
use solidcraft_kernel::Continuity;

use super::CommandSpec;
use super::features::add_feature;
use crate::params::{bad, str_, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("surface.match", "Match Surface", match_surface)
        .at("SURFACE", "MODIFY")
        .icon("continuity")
        .params(
            "body: surface name (the body whose face is rebuilt); faces?: [[x,y,z]] (points on that face, default: the body's first); target: body name (the body the seam must meet); edges: [[x,y,z]] (points on one edge chain of the target: the rebuilt boundary takes its place); order?: g1|g2|g3 (default g2: position and tangent plane, the curvature too, or how that curvature changes along the seam). The far boundary stays where it was and the body keeps its name",
        ),
];

/// `surface.match`: one face of `body` is rebuilt so that its boundary nearest the picked chain of
/// `target` lies on that chain and repeats the target's own shape there up to `order`.
fn match_surface(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.match";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let target = str_(p, "target").ok_or_else(|| bad(cmd, "`target` is required"))?.to_string();
    if s.model.state().body(&target).is_none() {
        return Err(bad(cmd, format!("no body `{target}`")));
    }
    let faces: Vec<solidcraft_geom::Vec3> = p.get("faces").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if faces.len() > 64 {
        return Err(bad(cmd, "too many points on the face to rebuild"));
    }
    let edges: Vec<solidcraft_geom::Vec3> = p.get("edges").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if edges.is_empty() || edges.len() > 10_000 {
        return Err(bad(cmd, "`edges` must list 1 to 10000 points on the target's edge chain"));
    }
    let order = match p.get("order") {
        None => Continuity::G2,
        Some(v) => v.as_str().and_then(Continuity::parse).filter(|o| *o != Continuity::G0).ok_or_else(|| bad(cmd, "`order` must be g1, g2 or g3"))?,
    };
    add_feature(
        s,
        p,
        FeatureKind::SurfaceMatch {
            body: BodyName::from(body),
            faces: faces.into_iter().map(FacePick).collect(),
            target: BodyName::from(target),
            edges: edges.into_iter().map(Position).collect(),
            order,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;
    use serde_json::json;
    use solidcraft_geom::Vec3;

    fn run(s: &mut Session, c: &str, p: Value) -> Value {
        s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"))
    }

    /// The picked chain: two points of the cylinder's own vertical wall seam, clear of the corners
    /// at either end. The seam is where `solid.extrude` starts the circle, so it is read off the
    /// body rather than assumed.
    fn picks(s: &Session) -> Vec<Vec3> {
        let boss = s.world_state().unwrap().body("Boss").unwrap().body.clone();
        let e = boss
            .edges(boss.size() * 1e-2)
            .unwrap()
            .into_iter()
            .find(|e| e.length > 25.0 && (e.mid.x.hypot(e.mid.y) - 10.0).abs() < 0.05 && e.mid.z > 5.0 && e.mid.z < 25.0)
            .expect("the cylinder has a straight vertical wall seam");
        let (a, b) = (e.points.first().copied(), e.points.last().copied());
        let (a, b) = (a.unwrap_or(e.mid), b.unwrap_or(e.mid));
        vec![a + (b - a) * 0.25, a + (b - a) * 0.75]
    }

    /// A cylinder of radius 10, 30 high, and a cylindrical skirt of radius 11 from the cylinder's
    /// own wall seam round 60 degrees: the skirt's near boundary is 1 mm off the seam line.
    fn boss_and_skirt(s: &mut Session) {
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
            .as_str()
            .unwrap()
            .to_string();
        run(s, "sketch.finish", json!({}));
        run(s, "surface.extrude", json!({"sketch": "Skirt sketch", "curves": [arc], "distance": 30, "body_name": "Skirt"}));
    }

    /// The seam of a matched skirt, measured back off the two bodies: how far the rebuilt boundary
    /// is from the picked seam line, the angle between the two surfaces' normals across it, and
    /// the continuity it holds.
    fn seam(s: &Session) -> (f64, f64, Continuity) {
        let st = s.world_state().unwrap();
        let made = st.body("Skirt").unwrap().body.clone();
        let target = st.body("Boss").unwrap().body.clone();
        solidcraft_kernel::seam_continuity(&made, &target, &picks(s)).unwrap()
    }

    /// `surface.match`: a skirt meets a cylinder's wall seam at g2; the body keeps its name, undo
    /// and redo move it back and forth, a cold reopen gives the same seam, and what cannot be
    /// matched is refused.
    #[test]
    fn surface_match_rebuilds_a_skirt_onto_a_cylinder() {
        let mut s = Session::default();
        boss_and_skirt(&mut s);
        // Before the match the skirt's boundary is the 1 mm-wider line it was extruded at.
        let (gap, _, _) = seam(&s);
        assert!((gap - 1.0).abs() < 0.05, "the skirt starts a millimetre off the seam: {gap}");
        let chain = picks(&s);
        run(&mut s, "surface.match", json!({"body": "Skirt", "target": "Boss", "edges": chain, "order": "g2"}));
        let st = s.world_state().unwrap();
        assert!(st.body("Skirt").is_some(), "the matched body keeps its name");
        assert!(st.body("Boss").is_some(), "the body it was matched to stays too");
        assert!(st.body("Skirt").unwrap().body.is_surface(), "a match stays a surface");
        let (gap, deg, order) = seam(&s);
        println!("engine: gap {gap:e} mm, normal {deg:e} deg, order {order:?}");
        assert!(gap < 1e-6, "the seam is on the picked line: {gap:e} mm");
        assert!(deg < 0.05, "the seam holds the wall's tangent plane: {deg} deg");
        assert!(order.conditions() >= Continuity::G1.conditions(), "{order:?}");
        // Undo puts the boundary back, redo matches it again, and a cold reopen agrees.
        run(&mut s, "edit.undo", json!({}));
        assert!(seam(&s).0 > 0.5, "undo restores the original skirt");
        run(&mut s, "edit.redo", json!({}));
        assert!(seam(&s).1 < 0.05, "redo matches it again");
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!(seam(&cold).1 < 0.05, "a reopened design matches the same");
        // Refusals: an unknown body, no target, no picks on the target, a word that is no order,
        // and a body that is not a surface.
        assert!(s.execute("surface.match", &json!({"body": "Nope", "target": "Boss", "edges": picks(&s)})).is_err());
        assert!(s.execute("surface.match", &json!({"body": "Skirt", "edges": picks(&s)})).is_err());
        assert!(s.execute("surface.match", &json!({"body": "Skirt", "target": "Boss", "edges": []})).is_err());
        assert!(s.execute("surface.match", &json!({"body": "Skirt", "target": "Nope", "edges": picks(&s)})).is_err());
        assert!(s.execute("surface.match", &json!({"body": "Skirt", "target": "Boss", "edges": picks(&s), "order": "g5"})).is_err());
        assert!(s.execute("surface.match", &json!({"body": "Skirt", "target": "Boss", "edges": [[100, 0, 0]]})).is_err());
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "B"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [4, 4]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "B", "distance": 4, "body_name": "Block"}));
        assert!(s.execute("surface.match", &json!({"body": "Block", "target": "Boss", "edges": picks(&s)})).is_err(), "a solid is not a surface");
    }
}
