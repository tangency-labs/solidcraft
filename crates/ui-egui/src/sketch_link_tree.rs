//! A sketch's projections (links to model geometry) are children of their sketch, beside its
//! patterns: one row per link naming its source and the placement it reads, its status when it
//! is not current, an eye that hides it, and a menu to update, freeze, remove or break it. The
//! rows require Project's session-only Projection display and picking option, read the stored
//! document and run commands.

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::sketch::{Link, LinkKind, LinkSource};
use solidcraft_theme::Tokens;

use crate::{
    SolveApp,
    browser::{Row, Spec, node_row},
    context_menu::Target,
    ui_kit::browser::Rows,
};

/// Links a sketch lists: references, not its text or the axes its constraints use.
pub(crate) fn listed(link: &Link) -> bool {
    link.kind != LinkKind::Text && !matches!(link.source, LinkSource::SketchAxis { .. })
}

pub(crate) fn has_links(app: &SolveApp, sketch: u64) -> bool {
    app.ui.project_link_tools && app.session.doc.sketch(sketch).is_ok_and(|sk| sk.links.iter().any(listed))
}

/// What a link's row says: its source (its kind too when it is not a projection, which the
/// icon already says) and the placement it reads.
fn label(app: &SolveApp, link: &Link) -> String {
    let what = link.source.describe();
    let mut label = match link.kind {
        LinkKind::Intersect => format!("{}: {what}", solidcraft_ui_text::tr_label("Intersect")),
        LinkKind::Include => format!("{}: {what}", solidcraft_ui_text::tr_label("Include 3D Geometry")),
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
/// waiting for an uncaptured move. An exploded view's placements are shown, not moves.
fn status(app: &SolveApp, sketch: u64, link: &Link) -> Option<&'static str> {
    let shown = app.session.model.result(sketch).and_then(|r| r.state.sketch(sketch)).and_then(|ss| ss.sketch.link(&link.id).map(|l| l.lost));
    if link.frozen {
        Some("Frozen")
    } else if shown.unwrap_or(link.lost) {
        Some("Lost")
    } else if link.placement.iter().flat_map(|p| p.source.iter().chain(&p.author)).any(|o| app.session.uncaptured_moves().contains_key(o)) {
        Some("Pending")
    } else {
        None
    }
}

/// One projection row of a sketch, as the Browser and the vertical feature history list it.
struct Entry {
    link: String,
    text: String,
    hidden: bool,
    lost: bool,
    selected: bool,
    /// What a click on it selects while its sketch is edited: its curves and points.
    members: Vec<Sel>,
}

fn entries(app: &SolveApp, sketch: u64) -> Vec<Entry> {
    if !app.ui.project_link_tools {
        return Vec::new();
    }
    let Ok(sk) = app.session.doc.sketch(sketch) else { return Vec::new() };
    let active = app.session.active_sketch == Some(sketch);
    sk.links
        .iter()
        .filter(|l| listed(l))
        .map(|link| {
            let mut text = label(app, link);
            let state = status(app, sketch, link);
            if let Some(state) = state {
                text.push_str(&format!(" — {}", solidcraft_ui_text::tr_label(state)));
            }
            let curves =
                sk.link_curves(&link.id).into_iter().filter_map(|c| sk.curves.get(c)).map(|c| Sel::SketchCurve { id: c.id.clone(), sketch: None });
            let points = sk.link_points(&link.id).into_iter().filter_map(|p| sk.points.get(p)).map(|p| Sel::SketchPoint { id: p.id.clone() });
            let members: Vec<Sel> = curves.chain(points).collect();
            let selected = active && !members.is_empty() && members.iter().all(|m| app.session.selection.contains(m));
            Entry { link: link.id.clone(), text, hidden: link.hidden, lost: state == Some("Lost"), selected, members }
        })
        .collect()
}

/// The row's look: the eye shows whether the projection is drawn, a lost one is in the error colour.
fn row<'a>(entry: &'a Entry, depth: usize, dim: bool) -> Row<'a> {
    Row {
        depth,
        eye: Some(!entry.hidden),
        icon: "project",
        label: &entry.text,
        name: true,
        selected: entry.selected,
        dim: dim || entry.hidden,
        color: entry.lost.then_some(Tokens::get().error),
        ..Default::default()
    }
}

/// A projection row's events: the eye shows or hides it, a click selects its members while the
/// sketch is edited (else the sketch), a secondary click opens its menu.
fn act(app: &mut SolveApp, sketch: u64, entry: Entry, (eye, clicked, secondary): (bool, bool, Option<egui::Pos2>)) {
    if eye {
        let _ = app.run("sketch.link_visibility", json!({"sketch":sketch,"link":entry.link,"visible":entry.hidden}));
    } else if clicked {
        let items = if app.session.active_sketch == Some(sketch) { entry.members } else { vec![Sel::Feature { id: sketch }] };
        let _ = app.run("select.set", json!({"items":items}));
    }
    if let Some(at) = secondary {
        crate::context_menu::open_for(app, at, Target::SketchLink { sketch, link: entry.link });
    }
}

/// The projections under their sketch in the Browser, beside its patterns. `dim`: the sketch is
/// hidden (or a placement above it), so its projections fade with it.
pub(crate) fn browser_rows(app: &mut SolveApp, rows: &mut Rows<'_>, sketch: u64, depth: usize, dim: bool) {
    for entry in entries(app, sketch) {
        let key = format!("sketch-link:{sketch}:{}", entry.link);
        let hit = node_row(rows, Spec { key: &key, ..Default::default() }, row(&entry, depth, dim));
        if let Some(rect) = hit.rect {
            crate::scenario::publish_handle(&format!("browser:sketch-link:{sketch}:{}", entry.link), rect.center());
        }
        act(app, sketch, entry, (hit.eye, hit.clicked, hit.secondary));
    }
}

/// The projections under their sketch in the vertical feature history.
pub(crate) fn rows(app: &mut SolveApp, ui: &mut egui::Ui, sketch: u64, depth: usize, scope: &str, dim: bool) {
    for entry in entries(app, sketch) {
        let response = crate::ui_kit::tree_row(ui, ui.scope_id().with(("sketch_link", sketch, &entry.link)), &row(&entry, depth, dim));
        if let Some(rect) = response.rect {
            crate::scenario::publish_handle(&format!("{scope}:sketch-link:{sketch}:{}", entry.link), rect.center());
        }
        act(app, sketch, entry, (response.eye, response.clicked, response.secondary));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    /// Part A (a holed plate) placed twice, A:2 at x = 100; a root sketch projecting A:2's top
    /// face and one of its edges. Returns (harness, sketch, face link, edge link, A:2).
    fn harness(enhanced: bool) -> (Harness, u64, String, String, u64) {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.ui.show_browser = true;
        h.app.ui.project_link_tools = enhanced;
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
    fn projection_compatibility_inputs_follow_the_project_option_and_explicit_inputs_win() {
        for enhanced in [false, true] {
            for policy in [None, Some(false), Some(true)] {
                let mut h = Harness::new();
                h.app.home.open = false;
                h.app.ui.project_link_tools = enhanced;
                let a = h.app.run("component.create", json!({"name":"A"})).unwrap();
                let a1 = a["occurrence"].as_u64().unwrap();
                let source = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
                h.app.run("sketch.line", json!({"points":[[2,0],[8,0]],"ids":["source"]})).unwrap();
                h.app.run("sketch.finish", json!({})).unwrap();
                let a2 = h.app.run("occurrence.copy", json!({"occurrence":a1})).unwrap()["occurrence"].as_u64().unwrap();
                h.app.run("occurrence.move", json!({"occurrence":a2,"translate":[50,0,0]})).unwrap();
                h.app.run("component.activate", json!({"component":0})).unwrap();
                h.app.run("component.create", json!({"name":"B"})).unwrap();
                let receiver = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
                let mut params = json!({"refs":[{"sketch":source,"curve":"source"}]});
                if let Some(policy) = policy {
                    params["strict_occurrence_references"] = json!(policy);
                }
                if policy == Some(true) {
                    // An explicit picked copy always wins over the default displayed placement.
                    params["refs"][0]["path"] = json!([a2]);
                }
                let r = h.app.run("sketch.project", params).unwrap();
                let link = r["links"][0]["link"].as_str().unwrap();
                let expected = if policy == Some(true) {
                    Some(vec![a2])
                } else if policy.is_none() && enhanced {
                    Some(vec![a1])
                } else {
                    None
                };
                let stored = h.app.session.doc.sketch(receiver).unwrap().link(link).unwrap();
                assert_eq!(stored.placement.as_ref().map(|p| p.source.clone()), expected);
                h.app.run("sketch.finish", json!({})).unwrap();
                let mut reopened = solidcraft_engine::Session::default();
                reopened.doc = solidcraft_engine::doc::format::read(&h.app.session.doc.to_json()).unwrap().into();
                reopened.refresh();
                let st = reopened.model.state();
                let l = st.sketch(receiver).unwrap().sketch.link(link).unwrap();
                assert_eq!(l.placement.as_ref().map(|p| p.source.clone()), expected);
                assert!(!l.lost);
            }
        }
    }

    #[test]
    fn partial_projection_delete_keeps_the_ui_choice_through_confirmation_and_reopen() {
        for enhanced in [false, true] {
            let (mut h, sketch, face, _, _) = harness(enhanced);
            h.app.apply_off_thread = None;
            // Extrusion needs the complete projected perimeter. Deleting a side makes the
            // downstream extrusion fail, so the real dry-run must ask for confirmation.
            h.app.run("solid.extrude", json!({"sketch":sketch,"distance":5,"operation":"new","body_name":"Receiver solid"})).unwrap();
            h.app.run("sketch.edit", json!({"sketch":sketch})).unwrap();
            let sk = h.app.session.doc.sketch(sketch).unwrap();
            let victim =
                sk.link_curves(&face).into_iter().find(|c| matches!(sk.curves[*c].kind, solidcraft_engine::sketch::CurveKind::Line { .. })).unwrap();
            let victim = sk.curves[victim].id.clone();
            let before = h.app.session.doc.to_json();
            crate::delete::request(&mut h.app, json!({"items":[{"type":"sketch_curve","id":victim}]}));
            let pending = h.app.menu.confirm.as_ref().expect("the extrusion depends on this closed boundary");
            assert_eq!(pending.params["preserve_projection_link"], enhanced);
            assert_eq!(h.app.session.doc.to_json(), before, "the dry-run changes nothing");
            h.app.ui.project_link_tools = !enhanced;
            assert!(crate::delete::confirm(&mut h.app, true));
            let sk = h.app.session.doc.sketch(sketch).unwrap();
            assert!(sk.curve_index(&victim).is_none());
            assert_eq!(sk.link(&face).is_some(), enhanced, "confirmation uses the choice at request time");
            let mut reopened = solidcraft_engine::Session::default();
            reopened.doc = solidcraft_engine::doc::format::read(&h.app.session.doc.to_json()).unwrap().into();
            reopened.refresh();
            assert_eq!(reopened.doc.sketch(sketch).unwrap().link(&face).is_some(), enhanced);
            assert!(reopened.doc.sketch(sketch).unwrap().curve_index(&victim).is_none());
        }
    }

    #[test]
    fn direct_ui_delete_honours_explicit_preservation_in_both_project_option_modes() {
        for enhanced in [false, true] {
            for policy in [None, Some(false), Some(true)] {
                let (mut h, sketch, face, _, _) = harness(enhanced);
                h.app.run("sketch.edit", json!({"sketch":sketch})).unwrap();
                let sk = h.app.session.doc.sketch(sketch).unwrap();
                let victim = sk
                    .link_curves(&face)
                    .into_iter()
                    .find(|c| matches!(sk.curves[*c].kind, solidcraft_engine::sketch::CurveKind::Line { .. }))
                    .unwrap();
                let mut p = json!({"entities":[sk.curves[victim].id]});
                if let Some(policy) = policy {
                    p["preserve_projection_link"] = json!(policy);
                }
                h.app.run("sketch.delete", p).unwrap();
                assert_eq!(h.app.session.doc.sketch(sketch).unwrap().link(&face).is_some(), policy.unwrap_or(enhanced));
            }
        }
    }

    #[test]
    fn projection_rows_and_folds_require_the_project_option_in_browser_and_history() {
        for left_history in [false, true] {
            for with_pattern in [false, true] {
                let (mut h, sketch, face, edge, _) = harness(false);
                h.app.run("view.history_layout", json!({"dock":if left_history { "left" } else { "bottom" }})).unwrap();
                let pattern = with_pattern.then(|| {
                    h.app.edit_sketch(sketch);
                    h.app.run("sketch.line", json!({"points":[[0,60],[10,60]],"ids":["source"],"connect":false,"infer":false})).unwrap();
                    let made = h.app.run("sketch.pattern.rectangular", json!({"entities":["source"],"count":2,"spacing":20})).unwrap();
                    h.app.finish_sketch();
                    made["pattern"].as_str().unwrap().to_string()
                });
                let before = serde_json::to_value(h.app.session.doc.sketch(sketch).unwrap()).unwrap();
                // Start with the default, enable, then disable again with both folds already open.
                for enhanced in [false, true, false, true] {
                    assert!(h.call("ui.set", json!({"projectLinkTools":enhanced}))["ok"].as_bool().unwrap());
                    h.frames(6);
                    assert!(crate::scenario::handle_at("row:Receiver").is_some(), "browser sketch remains visible");
                    assert_eq!(
                        crate::scenario::handle_at("fold:Receiver").is_some(),
                        with_pattern || enhanced,
                        "browser fold: left_history={left_history}, pattern={with_pattern}, option={enhanced}"
                    );
                    if left_history {
                        assert!(crate::scenario::rect_of("history:feature:Receiver").is_some(), "history sketch remains visible");
                        assert_eq!(
                            crate::scenario::handle_at("timeline-sketch-fold:Receiver").is_some(),
                            with_pattern || enhanced,
                            "history fold: pattern={with_pattern}, option={enhanced}"
                        );
                    }
                    for scope in if left_history { &["browser", "timeline"][..] } else { &["browser"][..] } {
                        for link in [&face, &edge] {
                            let handle = format!("{scope}:sketch-link:{sketch}:{link}");
                            assert_eq!(crate::scenario::handle_at(&handle).is_some(), enhanced, "{handle}, option={enhanced}");
                        }
                        if let Some(pattern) = &pattern {
                            assert!(crate::scenario::handle_at(&format!("{scope}:sketch-pattern:{sketch}:{pattern}")).is_some());
                        }
                    }
                    assert_eq!(serde_json::to_value(h.app.session.doc.sketch(sketch).unwrap()).unwrap(), before, "display changes no geometry");
                }
            }
        }
    }

    #[test]
    fn projection_display_option_does_not_change_the_constraint_command_policy() {
        for enhanced in [false, true] {
            let (mut h, sketch, _, edge, _) = harness(enhanced);
            h.app.edit_sketch(sketch);
            let sk = h.app.session.doc.sketch(sketch).unwrap();
            let curves = sk.link_curves(&edge);
            assert_eq!(curves.len(), 1, "the projected edge is one horizontal line");
            let line = sk.curves[curves[0]].id.clone();
            let count = sk.constraints.len();
            let result =
                h.call("engine.execute", json!({"command":"sketch.constraint.horizontal_vertical","params":{"line":line,"mode":"horizontal"}}));
            assert_eq!(result["ok"], true, "{result}");
            assert_eq!(result["result"]["redundant"], true, "{result}");
            assert_eq!(h.app.session.doc.sketch(sketch).unwrap().constraints.len(), count + 1);
            let before = serde_json::to_value(h.app.session.doc.sketch(sketch).unwrap()).unwrap();
            for params in [json!({"line":line,"mode":"horizontal","if_redundant":"refuse"}), json!({"line":line,"mode":"vertical"})] {
                let result = h.call("engine.execute", json!({"command":"sketch.constraint.horizontal_vertical","params":params}));
                assert_eq!(result["ok"], false, "{result}");
                assert_eq!(serde_json::to_value(h.app.session.doc.sketch(sketch).unwrap()).unwrap(), before);
            }
        }
    }

    #[test]
    fn projections_are_listed_under_their_sketch_with_their_placement() {
        let (h, sketch, face, edge, _) = harness(true);
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
        let (mut h, sketch, face, _, _) = harness(true);
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
        let (mut h, sketch, face, edge, a2) = harness(true);
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
        for another_copy in [false, true] {
            let (mut h, sketch, face, _, a2) = harness(true);
            let component = h.app.session.doc.occurrences.iter().find(|o| o.id == a2).unwrap().component;
            let a1 = h.app.session.doc.occurrences.iter().find(|o| o.component == component && o.id != a2).unwrap().id;
            let sk = h.app.session.doc.sketch(sketch).unwrap();
            let ids: Vec<String> = sk.link_curves(&face).iter().map(|c| sk.curves[*c].id.clone()).collect();
            h.app.run("selection.delete", json!({"items":[{"type":"occurrence","path":[a2]}]})).unwrap();
            if another_copy {
                let a3 = h.app.run("occurrence.copy", json!({"occurrence":a1})).unwrap()["occurrence"].as_u64().unwrap();
                h.app.run("occurrence.move", json!({"occurrence":a3,"translate":[0,100,0]})).unwrap();
            }
            h.app.edit_sketch(sketch);
            h.frames(3);
            let target = Target::SketchLink { sketch, link: face.clone() };
            let repair =
                crate::context_menu::items(&h.app, &target).into_iter().find(|i| i.id == "ui.repairLink").expect("a lost link offers Repair");
            assert!(repair.enabled);
            crate::context_menu::run_item(&mut h.app, &repair, egui::Pos2::ZERO);
            assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.repair_link"));
            // A:1's top face, the same shape as the one that went.
            h.settle();
            h.app.cam.target = solidcore_record::Vec3::new(20.0, 20.0, 10.0);
            h.app.cam.distance = 200.0;
            let at = h.call("ui.at", json!({"world":[5,5,10]}))["result"].clone();
            h.step(&json!({ "click": at })).unwrap();
            h.frames(3);
            let sk = h.app.session.doc.sketch(sketch).unwrap();
            assert_eq!(status(&h.app, sketch, sk.link(&face).unwrap()), None, "{:?}", h.app.status);
            if another_copy {
                assert_eq!(sk.link(&face).unwrap().placement.as_ref().unwrap().source, vec![a1], "it follows the picked A:1");
                assert!(label(&h.app, sk.link(&face).unwrap()).ends_with("(A:1)"));
            } else {
                assert!(sk.link(&face).unwrap().placement.is_none(), "a single remaining placement uses an ordinary link");
                assert_eq!(label(&h.app, sk.link(&face).unwrap()), "Face of Plate", "the ordinary source has no occurrence suffix");
            }
            let now: Vec<String> = sk.link_curves(&face).iter().map(|c| sk.curves[*c].id.clone()).collect();
            assert_eq!(now, ids, "repair keeps the projected curve identities");
            assert!(h.app.tool.is_none(), "the tool ends once repaired");
        }
    }
}
