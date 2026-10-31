//! A sketch's projections (links to model geometry) are children of their sketch, beside its
//! patterns: one row per link naming its source and the placement it reads, its status when it
//! is not current, an eye that hides it, and a menu to update, freeze, remove or break it. The
//! rows only read the stored document and run commands.

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::sketch::{Link, LinkKind, LinkSource};

use crate::{
    SolveApp,
    browser::{Row, draw_row},
    context_menu::Target,
    theme::Tokens,
};

/// Links a sketch lists: references, not its text or the axes its constraints use.
fn listed(link: &Link) -> bool {
    link.kind != LinkKind::Text && !matches!(link.source, LinkSource::SketchAxis { .. })
}

pub(crate) fn has_links(app: &SolveApp, sketch: u64) -> bool {
    app.session.doc.sketch(sketch).is_ok_and(|sk| sk.links.iter().any(listed))
}

/// What a link's row says: its source (its kind too when it is not a projection, which the
/// icon already says) and the placement it reads.
fn label(app: &SolveApp, link: &Link) -> String {
    let what = link.source.describe();
    let mut label = match link.kind {
        LinkKind::Intersect => format!("{}: {what}", crate::i18n::tr_label("Intersect")),
        LinkKind::Include => format!("{}: {what}", crate::i18n::tr_label("Include 3D Geometry")),
        _ => {
            let mut chars = what.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
        }
    };
    if let Some(p) = &link.placement
        && !p.source.is_empty()
    {
        label.push_str(&format!(" ({})", solidcraft_engine::doc::project::path_label(&app.session.doc, &p.source)));
    }
    label
}

/// The link's state when it is not simply current: lost (as the model shows it), frozen, or
/// waiting for an uncaptured move.
fn status(app: &SolveApp, sketch: u64, link: &Link) -> Option<&'static str> {
    let shown = app.session.model.result(sketch).and_then(|r| r.state.sketch(sketch)).and_then(|ss| ss.sketch.link(&link.id).map(|l| l.lost));
    if link.frozen {
        Some("Frozen")
    } else if shown.unwrap_or(link.lost) {
        Some("Lost")
    } else if link.placement.iter().flat_map(|p| p.source.iter().chain(&p.author)).any(|o| app.session.pending_moves.contains_key(o)) {
        Some("Pending")
    } else {
        None
    }
}

/// Shared by the parts browser and the vertical feature history, as the pattern rows are.
/// `dim`: the sketch is hidden (or a placement above it), so its projections fade with it.
pub(crate) fn rows(app: &mut SolveApp, ui: &mut egui::Ui, sketch: u64, depth: usize, scope: &str, dim: bool) {
    let links: Vec<Link> = app.session.doc.sketch(sketch).map(|sk| sk.links.iter().filter(|l| listed(l)).cloned().collect()).unwrap_or_default();
    let t = Tokens::get();
    for link in links {
        let mut text = label(app, &link);
        let state = status(app, sketch, &link);
        if let Some(state) = state {
            text.push_str(&format!(" — {}", crate::i18n::tr_label(state)));
        }
        let active = app.session.active_sketch == Some(sketch);
        let members: Vec<Sel> = app
            .session
            .doc
            .sketch(sketch)
            .map(|sk| {
                let curves = sk
                    .link_curves(&link.id)
                    .into_iter()
                    .filter_map(|c| sk.curves.get(c))
                    .map(|c| Sel::SketchCurve { id: c.id.clone(), sketch: None });
                let points = sk.link_points(&link.id).into_iter().filter_map(|p| sk.points.get(p)).map(|p| Sel::SketchPoint { id: p.id.clone() });
                curves.chain(points).collect()
            })
            .unwrap_or_default();
        let selected = active && !members.is_empty() && members.iter().all(|m| app.session.selection.contains(m));
        let response = draw_row(
            ui,
            ui.id().with(("sketch_link", sketch, &link.id)),
            &Row {
                depth,
                eye: Some(!link.hidden),
                icon: "project",
                label: &text,
                name: true,
                selected,
                dim: dim || link.hidden,
                color: (state == Some("Lost")).then_some(t.error),
                ..Default::default()
            },
        );
        if let Some(rect) = response.rect {
            crate::scenario::publish_handle(&format!("{scope}:sketch-link:{sketch}:{}", link.id), rect.center());
        }
        if response.eye {
            let _ = app.run("sketch.link_visibility", json!({"sketch":sketch,"link":link.id,"visible":link.hidden}));
        } else if response.clicked {
            let items = if active { members } else { vec![Sel::Feature { id: sketch }] };
            let _ = app.run("select.set", json!({"items":items}));
        }
        if let Some(at) = response.secondary {
            crate::context_menu::open_for(app, at, Target::SketchLink { sketch, link: link.id.clone() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    /// Part A (a holed plate) placed twice, A:2 at x = 100; a root sketch projecting A:2's top
    /// face and one of its edges. Returns (harness, sketch, face link, edge link, A:2).
    fn harness() -> (Harness, u64, String, String, u64) {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.ui.show_browser = true;
        let a1 = h.app.run("component.create", json!({"name":"A"})).unwrap()["occurrence"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length":40,"width":40,"height":10,"body_name":"Plate"})).unwrap();
        h.app.run("solid.cylinder", json!({"radius":5,"height":10,"base":[20,20,0],"operation":"cut","targets":["Plate"]})).unwrap();
        let a2 = h.app.run("occurrence.copy", json!({"occurrence":a1})).unwrap()["occurrence"].as_u64().unwrap();
        h.app.run("occurrence.move", json!({"occurrence":a2,"translate":[100,0,0]})).unwrap();
        h.app.run("component.activate", json!({"component":0})).unwrap();
        h.app.run("sketch.create", json!({"plane":"XY","offset":20,"name":"Receiver"})).unwrap();
        let sketch = h.app.session.active_sketch.unwrap();
        // The edge first: projected after the face, it would already be in the sketch.
        let edge = h.app.run("sketch.project", json!({"refs":[{"edge":[110,0,10],"body":"Plate (A:2)"}]})).unwrap();
        let face = h.app.run("sketch.project", json!({"refs":[{"face":[105,5,10],"body":"Plate (A:2)"}]})).unwrap();
        h.app.finish_sketch();
        h.frames(6);
        let link = |v: &serde_json::Value| v["links"][0]["link"].as_str().unwrap().to_string();
        (h, sketch, link(&face), link(&edge), a2)
    }

    #[test]
    fn projections_are_listed_under_their_sketch_with_their_placement() {
        let (h, sketch, face, edge, _) = harness();
        let parent = crate::scenario::handle_at("row:Receiver").unwrap();
        for link in [&face, &edge] {
            let row = crate::scenario::handle_at(&format!("browser:sketch-link:{sketch}:{link}")).unwrap();
            assert!(row.y > parent.y, "projections belong below their sketch");
        }
        let sk = h.app.session.doc.sketch(sketch).unwrap();
        assert!(label(&h.app, sk.link(&face).unwrap()).ends_with("(A:2)"));
        assert_eq!(status(&h.app, sketch, sk.link(&face).unwrap()), None);
    }

    #[test]
    fn the_eye_hides_a_projection_without_touching_the_sketch() {
        let (mut h, sketch, face, _, _) = harness();
        let profiles = h.app.session.model.state().sketch(sketch).unwrap().profiles.len();
        let text = label(&h.app, h.app.session.doc.sketch(sketch).unwrap().link(&face).unwrap());
        h.step(&json!({"click":{"handle":format!("eye:{text}")}})).unwrap();
        h.frames(3);
        assert!(h.app.session.doc.sketch(sketch).unwrap().link(&face).unwrap().hidden);
        assert_eq!(h.app.session.model.state().sketch(sketch).unwrap().profiles.len(), profiles, "visibility only");
        h.step(&json!({"click":{"handle":format!("eye:{text}")}})).unwrap();
        h.frames(3);
        assert!(!h.app.session.doc.sketch(sketch).unwrap().link(&face).unwrap().hidden);
    }

    #[test]
    fn a_lost_source_is_marked_and_the_menu_manages_the_projection() {
        let (mut h, sketch, face, edge, a2) = harness();
        let target = Target::SketchLink { sketch, link: face.clone() };
        let ids: Vec<String> = crate::context_menu::items(&h.app, &target).into_iter().map(|i| i.id).collect();
        for id in ["sketch.update_references", "sketch.freeze_link", "sketch.link_visibility", "sketch.remove_link", "sketch.break_link"] {
            assert!(ids.iter().any(|x| x == id), "{id} in {ids:?}");
        }
        h.app.run("selection.delete", json!({"items":[{"type":"occurrence","path":[a2]}]})).unwrap();
        h.frames(3);
        let sk = h.app.session.doc.sketch(sketch).unwrap();
        assert_eq!(status(&h.app, sketch, sk.link(&face).unwrap()), Some("Lost"));
        // Remove from the menu: the projection's row goes, the other stays.
        let remove = crate::context_menu::items(&h.app, &target).into_iter().find(|i| i.id == "sketch.remove_link").unwrap();
        assert!(remove.enabled);
        h.app.run(&remove.id, remove.params.clone().unwrap()).unwrap();
        h.frames(3);
        assert!(crate::scenario::handle_at(&format!("browser:sketch-link:{sketch}:{face}")).is_none());
        assert!(crate::scenario::handle_at(&format!("browser:sketch-link:{sketch}:{edge}")).is_some());
        h.app.run("edit.undo", json!({})).unwrap();
        h.frames(3);
        assert!(crate::scenario::handle_at(&format!("browser:sketch-link:{sketch}:{face}")).is_some());
    }

    #[test]
    fn a_lost_projection_is_repaired_by_picking_its_new_source() {
        let (mut h, sketch, face, _, a2) = harness();
        h.app.run("selection.delete", json!({"items":[{"type":"occurrence","path":[a2]}]})).unwrap();
        h.app.edit_sketch(sketch);
        h.frames(3);
        let target = Target::SketchLink { sketch, link: face.clone() };
        let repair = crate::context_menu::items(&h.app, &target).into_iter().find(|i| i.id == "ui.repairLink").expect("a lost link offers Repair");
        assert!(repair.enabled);
        crate::context_menu::run_item(&mut h.app, &repair, egui::Pos2::ZERO);
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.repair_link"));
        // A:1's top face, the same shape as the one that went.
        h.settle();
        h.app.cam.target = solidcraft_engine::geom::Vec3::new(20.0, 20.0, 10.0);
        h.app.cam.distance = 200.0;
        let at = h.call("ui.at", json!({"world":[5,5,10]}))["result"].clone();
        h.step(&json!({ "click": at })).unwrap();
        h.frames(3);
        let sk = h.app.session.doc.sketch(sketch).unwrap();
        assert_eq!(status(&h.app, sketch, sk.link(&face).unwrap()), None, "{:?}", h.app.status);
        assert!(sk.link(&face).unwrap().placement.as_ref().is_some_and(|p| !p.source.contains(&a2)), "it follows A:1 now");
        assert!(h.app.tool.is_none(), "the tool ends once repaired");
    }
}
