use std::sync::Arc;

use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_doc::MAX_PLACEMENT_DEPTH;
use solidcraft_doc::drawing::DEFAULT_DPI;
use solidcraft_doc::drawing::view::{Diagonal, QuarterTurn};
use solidcraft_geom::cell::Projection;
use solidcraft_geom::drawing_view::ProjectedSide;
use solidcraft_geom::paper::NoPictures;
use solidcraft_geom::paper::line::{LineGroup, LineKind};

use super::*;
use crate::drawing_scene::tests::{
    add_a_pin, carcass_and_door, configured_block, hinge_pair, in_the_model, plate as sheet_plate, two_cubes, with_the_model,
};
use crate::drawing_view::bend_dashes_mm;
use crate::schema::tests::conforms;

const EPS: f64 = 1e-9;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap()
}

/// A block 100 x 50 x 20 mm along X, Y and Z, from the origin.
fn block() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 50, "height": 20, "body_name": "Block"}));
    s
}

fn snap(s: &mut Session, p: Value) -> Value {
    run(s, CMD, p)
}

fn refused(s: &mut Session, p: Value) -> String {
    s.execute(CMD, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"))
}

fn point(v: &Value) -> Vec2 {
    Vec2::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
}

/// The boundary's size and centre on the sheet.
fn boundary(v: &Value) -> (Vec2, Vec2) {
    let (min, max) = (point(&v["boundary_mm"]["min"]), point(&v["boundary_mm"]["max"]));
    (max - min, (min + max) * 0.5)
}

/// `v`'s boundary is the box from `min` to `max` on the sheet.
fn assert_box(v: &Value, min: [f64; 2], max: [f64; 2]) {
    let (lo, hi) = (point(&v["boundary_mm"]["min"]), point(&v["boundary_mm"]["max"]));
    assert!((lo - Vec2::from(min)).len() < EPS && (hi - Vec2::from(max)).len() < EPS, "{v}");
}

/// A drawing of the block: its front view at 1:2 drawing hidden lines too, the right view lined up
/// with it, and the right view's top view placed apart. The session and the three views' ids.
pub(crate) fn three_views() -> (Session, [u64; 3]) {
    three_views_of(block())
}

/// [`three_views`] of the design `s`.
pub(crate) fn three_views_of(mut s: Session) -> (Session, [u64; 3]) {
    run(&mut s, "drawing.create", json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    let mut id_source = solidcraft_doc::ids::IdSource::seeded(4816);
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    let mut add = |d: &mut Drawing, kind: ViewKind, place: ViewPlace, settings: ViewOverrides| {
        let id = d.allocate_id(&mut id_source).unwrap();
        d.add_views([Arc::new(View { id, sheet, kind, place, settings, reference_shown_percent: 0.0, centre_marks: None, label: None })]);
        id
    };
    let front_settings =
        ViewOverrides { scale: Some("1:2".parse().unwrap()), visual_style: Some(VisualStyle::VisibleHidden), ..ViewOverrides::default() };
    let front = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
    let front = add(&mut d, front, ViewPlace::At([150.0, 150.0]), front_settings);
    let right = add(&mut d, ViewKind::Projected { parent: front, side: ProjectedSide::Right }, ViewPlace::Aligned(300.0), ViewOverrides::default());
    let top = add(&mut d, ViewKind::Projected { parent: right, side: ProjectedSide::Above }, ViewPlace::At([300.0, 250.0]), ViewOverrides::default());
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    (s, [front, right, top])
}

fn inspect_view(s: &mut Session, view: u64) -> Value {
    let reply = run(s, INSPECT, json!({"view": ids::encode(view)}));
    let schema = output_schema_for::<InspectResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    reply
}

fn edit_view(s: &mut Session, view: u64, set: Value) -> Value {
    let reply = run(s, EDIT, json!({"view": ids::encode(view), "set": set}));
    let schema = output_schema_for::<EditResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    reply
}

fn set_projection(s: &mut Session, projection: Projection) {
    let mut d = (**s.drawing("test").unwrap()).clone();
    d.projection = projection;
    s.doc_mut().put_drawing(Arc::new(d));
}

/// Component `L{level}`, placed inside `L{level - 1}` (the design itself at level 0).
fn nest(s: &mut Session, level: u64) {
    let (component, parent) = (1000 + level, if level == 0 { 0 } else { 1000 + level - 1 });
    let doc = s.doc_mut();
    doc.components.push(solidcraft_doc::Component { id: component, name: format!("L{level}"), parent, ..Default::default() });
    let name = format!("L{level}");
    // From JSON, so a field another change adds to occurrences takes its default here.
    let occurrence = json!({"id": 5000 + level, "component": component, "parent": parent, "name": name});
    doc.occurrences.push(serde_json::from_value(occurrence).unwrap());
}

fn edit_refused(s: &mut Session, view: u64, set: Value) -> String {
    s.execute(EDIT, &json!({"view": ids::encode(view), "set": set})).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {set}"))
}

/// Each setting says its value, where it comes from (the view, a parent by id, or the default) and
/// what the view sets itself; a projected view says whether it follows its parent's display; the
/// direction is named only when it is exactly a named view's.
#[test]
fn inspect_says_where_each_setting_comes_from() {
    let (mut s, [front, right, top]) = three_views();
    let front_id = ids::encode(front);
    let base = inspect_view(&mut s, front);
    assert_eq!(
        (base["kind"].clone(), base["direction"]["named"].clone(), base["place"].clone()),
        (json!("base"), json!("front"), json!({"at": [150.0, 150.0]}))
    );
    assert_eq!(base["settings"]["scale"], json!({"value": "1:2", "from": "this", "set": "1:2"}));
    assert_eq!(base["settings"]["visual_style"], json!({"value": "visible_hidden", "from": "this", "set": "visible_hidden"}));
    assert_eq!(base["settings"]["reference_style"], json!({"value": "visible", "from": "default", "set": null}));
    assert!(base.get("linked").is_none() && base.get("parent").is_none(), "a base view has no parent");
    let projected = inspect_view(&mut s, right);
    assert_eq!(
        (projected["kind"].clone(), projected["parent"].clone(), projected["side"].clone()),
        (json!("projected"), json!(front_id), json!("right"))
    );
    assert_eq!(
        (projected["direction"]["named"].clone(), projected["place"].clone(), projected["linked"].clone()),
        (json!("right"), json!({"aligned": 300.0}), json!(true))
    );
    assert_eq!(projected["settings"]["scale"], json!({"value": "1:2", "from": "parent", "parent": front_id, "set": null}));
    assert_eq!(projected["source"], base["source"]);
    let chained = inspect_view(&mut s, top);
    assert_eq!(chained["settings"]["visual_style"], json!({"value": "visible_hidden", "from": "parent", "parent": front_id, "set": null}));
    assert!(chained["direction"].get("named").is_none(), "the right view's top view is no named view: {chained}");
    assert_eq!(chained["choices"]["visual_style"].as_array().unwrap().len(), 5);
    assert_eq!(chained["choices"]["scale"][0], "50:1");
    assert!(s.execute(INSPECT, &json!({"view": ids::encode(99)})).unwrap_err().to_string().contains("no view"));
}

/// An edit is one undo step that evaluates no model: it names what it changed and the views
/// projected from it whose settings changed with it; `null` follows the parent again; linking and
/// unlinking set or clear the display settings together; what isn't a view's to change is refused,
/// and an edit that changes nothing is no step.
#[test]
fn an_edit_is_one_undo_step_and_names_its_followers() {
    let (mut s, [front, right, top]) = three_views();
    let evaluations = s.evaluations;
    let steps = s.undo.len();
    let edited = edit_view(&mut s, right, json!({"visual_style": "visible"}));
    assert_eq!((edited["changed"].clone(), edited["followers"].clone()), (json!(["visual_style"]), json!([ids::encode(top)])));
    assert_eq!(s.undo.len(), steps + 1);
    assert_eq!(inspect_view(&mut s, right)["linked"], "partly");
    assert_eq!(inspect_view(&mut s, top)["settings"]["visual_style"]["parent"], json!(ids::encode(right)));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(inspect_view(&mut s, right)["settings"]["visual_style"]["from"], "parent");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(edit_view(&mut s, right, json!({"visual_style": null}))["changed"], json!(["visual_style"]));
    assert_eq!(inspect_view(&mut s, right)["settings"]["visual_style"]["from"], "parent", "null follows the parent again");
    // Unlinked, the display settings are its own at the values they had; linked, they follow again.
    edit_view(&mut s, right, json!({"linked": false}));
    let unlinked = inspect_view(&mut s, right);
    assert_eq!((unlinked["linked"].clone(), unlinked["settings"]["visual_style"]["set"].clone()), (json!(false), json!("visible_hidden")));
    edit_view(&mut s, right, json!({"linked": true}));
    assert_eq!(inspect_view(&mut s, right)["linked"], true);
    // A base view keeps a scale and has nothing to link to; a projected view's direction is its side's.
    assert!(edit_refused(&mut s, front, json!({"scale": null})).contains("keeps a scale"));
    assert!(edit_refused(&mut s, front, json!({"linked": true})).contains("no parent"));
    assert!(edit_refused(&mut s, right, json!({"view": "top"})).contains("only a base view's direction"));
    assert!(edit_refused(&mut s, front, json!({"place": {"aligned": 10.0}})).contains("a base view is placed at a point"));
    assert!(edit_refused(&mut s, front, json!({"dpi": 300.0})).contains("dpi"), "a resolution is the sheet's");
    assert!(edit_refused(&mut s, front, json!({"scale": "1:0"})).contains("scale"));
    assert!(edit_refused(&mut s, 99, json!({})).contains("no view"));
    // The front view turned to the top, and the right view placed apart.
    let turned = edit_view(&mut s, front, json!({"view": "top", "scale": "1:5"}));
    assert_eq!(turned["changed"], json!(["scale", "view"]));
    assert_eq!(turned["followers"], json!([ids::encode(top), ids::encode(right)]), "by id");
    assert_eq!(edit_view(&mut s, right, json!({"place": {"at": [320.0, 150.0]}}))["changed"], json!(["place", "label"]), "and given its letter");
    // The top view was placed apart without a letter (built above, not by a command): its first edit gives it one.
    let edited = edit_view(&mut s, top, json!({"reference_shown_percent": 5.0}));
    assert_eq!(edited["changed"], json!(["reference_shown_percent", "label"]));
    assert_eq!(inspect_view(&mut s, top)["reference_shown_percent"], 5.0);
    for percent in [-1.0, 100.001, 1e308] {
        assert!(edit_refused(&mut s, top, json!({"reference_shown_percent": percent})).contains("reference_shown_percent"), "{percent}");
    }
    // Nothing changed, no step.
    let steps = s.undo.len();
    assert_eq!(edit_view(&mut s, front, json!({"scale": "1:5"}))["changed"], json!([]));
    assert_eq!(s.undo.len(), steps, "no change, no step");
    assert_eq!(s.evaluations, evaluations, "no model is evaluated");
}

/// An edit that moves a view or changes its size lists it, and the views lined up with it, when
/// they now leave the sheet's drawing space, as the commands that place a view do; none is moved
/// back in. An edit that changes no size or place lists nothing.
/// Standard views: the main view, its top view above and its right view to the right of it in third
/// angle (below and to the left in first), lined up with it, and an isometric view in the corner
/// between, at the largest preset at which all of them fit, a gap apart; one undo step. A scale too
/// large lists the views that leave the sheet; a bad gap is refused.
#[test]
fn standard_views_are_laid_out_together_in_the_drawings_projection() {
    let mut s = block();
    run(&mut s, "drawing.create", json!({}));
    let steps = s.undo.len();
    let reply = run(&mut s, STANDARD, json!({"sheet": "Sheet 1"}));
    let schema = output_schema_for::<StandardResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!(s.undo.len(), steps + 1, "one undo step");
    assert_eq!(reply["scale"]["fitted"], true);
    assert!(reply.get("outside").is_none(), "{reply}");
    let view = |s: &mut Session, key: &str| inspect_view(s, ids::decode(reply[key].as_str().unwrap()).unwrap());
    let (main, top, right, iso) = (view(&mut s, "view"), view(&mut s, "top"), view(&mut s, "right"), view(&mut s, "iso"));
    let min = |v: &Value| point(&v["boundary_mm"]["min"]);
    let max = |v: &Value| point(&v["boundary_mm"]["max"]);
    assert!((min(&top).y - max(&main).y - 20.0).abs() < 1e-6, "top view 20 mm above: {top}");
    assert!((min(&right).x - max(&main).x - 20.0).abs() < 1e-6, "right view 20 mm to the right: {right}");
    assert!((boundary(&top).1.x - boundary(&main).1.x).abs() < 1e-6 && (boundary(&right).1.y - boundary(&main).1.y).abs() < 1e-6, "lined up");
    assert!(min(&iso).x > max(&main).x && min(&iso).y > max(&main).y, "the iso view in the corner between: {iso}");
    assert_eq!((top["kind"].clone(), top["side"].clone(), right["side"].clone()), (json!("projected"), json!("above"), json!("right")));
    assert_eq!(iso["direction"]["named"], "iso_front_right");
    // The next larger preset doesn't fit: the scale is the largest that does.
    let at = PRESETS.iter().position(|p| p.to_string() == reply["scale"]["text"].as_str().unwrap()).unwrap();
    assert!(at > 0, "a 100 mm block fits A3 below 50:1");
    // First angle: the top view below, the right view to the left, and no iso view when asked.
    let mut first = block();
    run(&mut first, "drawing.create", json!({"projection": "first"}));
    let reply = run(&mut first, STANDARD, json!({"sheet": "Sheet 1", "iso": false, "gap_mm": 10}));
    assert!(reply.get("iso").is_none(), "{reply}");
    let view = |s: &mut Session, key: &str| inspect_view(s, ids::decode(reply[key].as_str().unwrap()).unwrap());
    let (main, top, right) = (view(&mut first, "view"), view(&mut first, "top"), view(&mut first, "right"));
    assert!((min(&main).y - max(&top).y - 10.0).abs() < 1e-6, "top view 10 mm below: {top}");
    assert!((min(&main).x - max(&right).x - 10.0).abs() < 1e-6, "right view 10 mm to the left: {right}");
    // A scale too large leaves the sheet, and says so; a gap out of range is refused.
    let huge = run(&mut first, STANDARD, json!({"sheet": "Sheet 1", "scale": "10:1"}));
    assert_eq!(huge["scale"]["fitted"], false);
    assert!(!huge["outside"].as_array().unwrap().is_empty(), "{huge}");
    for gap in [-1.0, 1000.5, 1e308] {
        let e = first.execute(STANDARD, &json!({"sheet": "Sheet 1", "gap_mm": gap})).unwrap_err().to_string();
        assert!(e.contains("gap_mm"), "{gap}: {e}");
    }
}

/// An edit that moves a view or changes its size lists it, and the views lined up with it, when
/// they now leave the sheet's drawing space, as the commands that place a view do; none is moved
/// back in. An edit that changes no size or place lists nothing.
#[test]
fn an_edit_that_takes_a_view_off_the_sheet_says_so() {
    let (mut s, [front, right, _]) = three_views();
    let scaled = edit_view(&mut s, front, json!({"scale": "1:1"}));
    assert!(scaled.get("outside").is_none(), "still on the sheet: {scaled}");
    // Up past the sheet's top edge, the front view and the right view lined up with it both leave.
    let off = edit_view(&mut s, front, json!({"place": {"at": [150.0, 290.0]}}));
    let listed: Vec<Value> = off["outside"].as_array().unwrap().iter().map(|o| o["view"].clone()).collect();
    assert_eq!(listed, vec![json!(ids::encode(front)), json!(ids::encode(right))], "{off}");
    assert_eq!(inspect_view(&mut s, front)["place"], json!({"at": [150.0, 290.0]}), "none is moved back in");
    // A display setting changes no size or place: nothing is listed.
    let styled = edit_view(&mut s, front, json!({"visual_style": "visible"}));
    assert!(styled.get("outside").is_none(), "{styled}");
}

/// A second sheet in the drawing, A3 landscape as the first: its id.
fn second_sheet(s: &mut Session) -> u64 {
    let added = run(s, "drawing.sheet.add", json!({}));
    ids::decode(added["sheet"].as_str().unwrap()).unwrap()
}

/// View `id` as the drawing holds it.
fn saved(s: &Session, id: u64) -> View {
    s.drawing("test").unwrap().views.iter().find(|v| v.id == id).map(|v| (**v).clone()).unwrap()
}

fn to_sheet(s: &mut Session, cmd: &str, p: Value) -> Value {
    let reply = run(s, cmd, p);
    let schema = output_schema_for::<ToSheetResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    reply
}

fn listed(reply: &Value) -> Vec<u64> {
    reply["views"].as_array().unwrap().iter().map(|v| ids::decode(v.as_str().unwrap()).unwrap()).collect()
}

/// A move is one undo step: the view and the views lined up with it go to the sheet and keep their
/// places beside it, and a view placed apart stays. Without a position it keeps its own; to its
/// own sheet it changes nothing.
#[test]
fn a_moved_view_takes_the_views_lined_up_with_it() {
    let (mut s, [front, right, top]) = three_views();
    let (first, second) = (saved(&s, front).sheet, second_sheet(&mut s));
    let steps = s.undo.len();
    let moved = to_sheet(&mut s, MOVE, json!({"view": ids::encode(front), "sheet": ids::encode(second), "position_mm": [100.0, 120.0]}));
    assert_eq!((listed(&moved), moved["sheet"].clone()), (vec![front, right], json!(ids::encode(second))));
    assert_eq!(s.undo.len(), steps + 1);
    assert_eq!((saved(&s, front).sheet, saved(&s, front).place), (second, ViewPlace::At([100.0, 120.0])));
    assert_eq!((saved(&s, right).sheet, saved(&s, right).place), (second, ViewPlace::Aligned(250.0)), "50 mm left, as its parent went");
    assert_eq!(saved(&s, top).sheet, first, "a view placed apart stays");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!((saved(&s, front).sheet, saved(&s, right).sheet, saved(&s, right).place), (first, first, ViewPlace::Aligned(300.0)));
    to_sheet(&mut s, MOVE, json!({"view": ids::encode(front), "sheet": "Sheet 2"}));
    assert_eq!((saved(&s, front).place, saved(&s, right).place), (ViewPlace::At([150.0, 150.0]), ViewPlace::Aligned(300.0)), "by name, where it was");
    let steps = s.undo.len();
    assert_eq!(to_sheet(&mut s, MOVE, json!({"view": ids::encode(front), "sheet": ids::encode(second)}))["views"], json!([]));
    assert_eq!(s.undo.len(), steps, "its own sheet: no step");
}

/// A view lined up with its parent moves only with `break_alignment`, and is then placed apart: at
/// the position, else the sheet's centre; the views lined up with it keep their places beside it.
/// What would break the record (an unknown view or sheet, a place off the paper) is refused, and
/// changes nothing.
#[test]
fn a_lined_up_view_moves_only_when_its_line_up_is_broken() {
    let (mut s, [front, right, top]) = three_views();
    let second = second_sheet(&mut s);
    let to = |view: u64, more: Value| {
        let mut p = json!({"view": ids::encode(view), "sheet": ids::encode(second)});
        p.as_object_mut().unwrap().extend(more.as_object().unwrap().clone());
        p
    };
    let refused = |s: &mut Session, p: Value| s.execute(MOVE, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"));
    assert!(refused(&mut s, to(right, json!({}))).contains("break_alignment"));
    // The top view lined up above the right view, whose centre is (300, 150).
    edit_view(&mut s, top, json!({"place": {"aligned": 250.0}}));
    let moved = to_sheet(&mut s, MOVE, to(right, json!({"break_alignment": true, "position_mm": [200.0, 100.0]})));
    assert_eq!(listed(&moved), [right, top]);
    assert_eq!(
        (saved(&s, right).place, saved(&s, top).place, saved(&s, top).sheet),
        (ViewPlace::At([200.0, 100.0]), ViewPlace::Aligned(200.0), second)
    );
    assert_eq!(saved(&s, front).sheet, s.drawing("test").unwrap().sheets[0].id, "its parent stays");
    run(&mut s, "edit.undo", json!({}));
    to_sheet(&mut s, MOVE, to(right, json!({"break_alignment": true})));
    assert_eq!((saved(&s, right).place, saved(&s, top).place), (ViewPlace::At([210.0, 148.5]), ViewPlace::Aligned(248.5)), "the A3 sheet's centre");
    run(&mut s, "edit.undo", json!({}));
    let before = s.drawing("test").unwrap().clone();
    assert!(refused(&mut s, to(99, json!({}))).contains("no view"));
    assert!(refused(&mut s, json!({"view": ids::encode(front), "sheet": "Sheet 9"})).contains("no sheet"));
    assert!(refused(&mut s, to(front, json!({"position_mm": [1e8, 0.0]}))).contains("5000"));
    assert!(refused(&mut s, to(front, json!({"position_mm": [4990.0, 150.0]}))).contains("5000"), "the right view would land off the paper");
    assert!(refused(&mut s, to(front, json!({"break_alignment": "yes"}))).contains("break_alignment"));
    assert_eq!(*s.drawing("test").unwrap(), before);
}

/// A view's line group is its own, its parent's, its sheet's or its drawing's, as inspect says. It
/// is a display setting: unlinking sets it at the value it has, linking clears it.
#[test]
fn a_views_line_group_follows_its_sheet_and_links_with_its_parent() {
    let (mut s, [front, right, top]) = three_views();
    assert_eq!(inspect_view(&mut s, front)["settings"]["line_group"], json!({"value": "0.5", "from": "drawing", "set": null}));
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    run(&mut s, "drawing.sheet.set", json!({"sheet": sheet, "line_group": "0.35"}));
    assert_eq!(inspect_view(&mut s, right)["settings"]["line_group"]["from"], "sheet");
    let edited = edit_view(&mut s, right, json!({"line_group": "0.7"}));
    assert_eq!((edited["changed"].clone(), edited["followers"].clone()), (json!(["line_group"]), json!([ids::encode(top)])));
    let followed = json!({"value": "0.7", "from": "parent", "parent": ids::encode(right), "set": null});
    assert_eq!(inspect_view(&mut s, top)["settings"]["line_group"], followed);
    assert_eq!(inspect_view(&mut s, right)["linked"], "partly");
    edit_view(&mut s, right, json!({"linked": true}));
    assert_eq!(inspect_view(&mut s, right)["settings"]["line_group"]["from"], "sheet", "linking clears it");
    edit_view(&mut s, right, json!({"linked": false}));
    assert_eq!(inspect_view(&mut s, right)["settings"]["line_group"]["set"], "0.35", "unlinking keeps the value it had");
    let why = edit_refused(&mut s, front, json!({"line_group": "0.6"}));
    assert!(why.ends_with("`set.line_group` must be one of 0.25, 0.35, 0.5, 0.7, 1, 1.4 or 2"), "{why}");
}

/// A copy is one undo step of new views, independent of the ones it came from: with its linked
/// views, they are copied lined up with the copy; without, it goes alone. Without a choice, a view
/// with views lined up with it is refused, naming them.
#[test]
fn a_copy_takes_its_linked_views_only_when_asked() {
    let (mut s, [front, right, top]) = three_views();
    let second = second_sheet(&mut s);
    let message = s.execute(COPY, &json!({"view": ids::encode(front), "sheet": ids::encode(second)})).unwrap_err().to_string();
    assert!(message.contains("with_linked") && message.contains(&ids::encode(right)), "{message}");
    let steps = s.undo.len();
    let p = json!({"view": ids::encode(front), "sheet": ids::encode(second), "position_mm": [100.0, 100.0], "with_linked": true});
    let copied = to_sheet(&mut s, COPY, p);
    let [copy, side] = listed(&copied)[..] else { panic!("{copied}") };
    assert_eq!(s.undo.len(), steps + 1);
    assert!(![front, right, top].contains(&copy) && ![front, right, top].contains(&side), "new ids");
    assert_eq!(copied["view"], json!(ids::encode(copy)));
    let (original, made) = (saved(&s, front), saved(&s, copy));
    assert_eq!((made.sheet, made.kind, made.settings, made.place), (second, original.kind, original.settings, ViewPlace::At([100.0, 100.0])));
    assert_eq!(saved(&s, side).kind, ViewKind::Projected { parent: copy, side: ProjectedSide::Right }, "lined up with the copy, not the original");
    assert_eq!((saved(&s, side).place, saved(&s, side).sheet), (ViewPlace::Aligned(250.0), second));
    assert_eq!((saved(&s, front).place, saved(&s, right).place), (ViewPlace::At([150.0, 150.0]), ViewPlace::Aligned(300.0)), "the originals stay");
    // An edit of the original changes nothing of the copies.
    edit_view(&mut s, front, json!({"scale": "1:5"}));
    assert_eq!(inspect_view(&mut s, side)["settings"]["scale"]["value"], "1:2");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.drawing("test").unwrap().views.len(), 3, "the undo takes both copies away");
    let alone = to_sheet(&mut s, COPY, json!({"view": ids::encode(front), "sheet": ids::encode(second), "with_linked": false}));
    assert_eq!(listed(&alone).len(), 1);
    assert_eq!(saved(&s, listed(&alone)[0]).place, ViewPlace::At([210.0, 148.5]), "the sheet's centre");
    // Beside the original, on its own sheet.
    let first = saved(&s, front).sheet;
    let beside = to_sheet(&mut s, COPY, json!({"view": ids::encode(top), "sheet": ids::encode(first), "position_mm": [60.0, 60.0]}));
    assert_eq!(saved(&s, listed(&beside)[0]).sheet, first);
}

/// A copy's ids come from the session's id source, as every drawing object's do: sessions seeded
/// alike copy a view and the view lined up with it to the same new ids (where a crash replay
/// starts), another seed to others.
#[test]
fn a_copys_ids_come_from_the_sessions_seed() {
    let copied = |seed: u64| {
        let (mut s, [front, ..]) = three_views();
        s.seed_ids(seed);
        let second = second_sheet(&mut s);
        let p = json!({"view": ids::encode(front), "sheet": ids::encode(second), "with_linked": true});
        listed(&to_sheet(&mut s, COPY, p))
    };
    assert_eq!(copied(7), copied(7));
    assert_ne!(copied(7), copied(8));
}

/// A projected view copied alone is a base view of its own: looking the way it did (by name when it
/// is exactly a named view, else by its line of sight and up), with the settings it resolved to as
/// its own, whatever its old parents do after.
#[test]
fn a_side_view_copied_alone_is_a_base_view_looking_the_way_it_did() {
    let (mut s, [front, right, top]) = three_views();
    let source = s.drawing("test").unwrap().sources[0].id;
    let copy = |s: &mut Session, view: u64| listed(&to_sheet(s, COPY, json!({"view": ids::encode(view), "sheet": "Sheet 1"})))[0];
    let of_right = copy(&mut s, right);
    let made = saved(&s, of_right);
    assert_eq!(made.kind, ViewKind::Base { source, view: ViewDirection::Named(NamedView::Right) });
    let own = ViewOverrides {
        scale: Some("1:2".parse().unwrap()),
        visual_style: Some(VisualStyle::VisibleHidden),
        reference_style: Some(VisualStyle::Visible),
        reference_hides_model: Some(false),
        tangent_edges: Some(Default::default()),
        ..ViewOverrides::default()
    };
    assert_eq!(made.settings, own);
    // A group its chain gave it is its own; one its sheet or drawing gave it, its new sheet gives.
    edit_view(&mut s, front, json!({"line_group": "0.7"}));
    let chained = copy(&mut s, right);
    assert_eq!(saved(&s, chained).settings.line_group, Some(LineGroup::Mm0_7));
    let of_top = copy(&mut s, top);
    let d = s.drawing("test").unwrap().clone();
    let ViewKind::Base { view: ViewDirection::Looking(_), .. } = saved(&s, of_top).kind else { panic!("the right view's top view is no named view") };
    let (was, is) = (d.resolve_view(top).unwrap().basis, d.resolve_view(of_top).unwrap().basis);
    for (a, b) in [(was.x, is.x), (was.y, is.y), (was.origin, is.origin)] {
        assert!((a - b).len() < 1e-12, "{a:?} {b:?}");
    }
    edit_view(&mut s, front, json!({"visual_style": "visible", "scale": "1:10"}));
    assert_eq!(saved(&s, of_right).settings, own, "the copy follows nothing");
}

/// `cmd` makes a view as `p` says, in one undo step: its reply, which matches its schema, and the
/// new view's id.
fn create(s: &mut Session, cmd: &str, p: Value) -> (Value, u64) {
    let steps = s.undo.len();
    let reply = run(s, cmd, p);
    assert_eq!(s.undo.len(), steps + 1, "one undo step");
    let schema = output_schema_for::<CreateResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    let id = ids::decode(reply["view"].as_str().unwrap()).unwrap();
    (reply, id)
}

/// `cmd` refuses `p`, leaving the drawing and the undo steps as they were: why.
fn create_refused(s: &mut Session, cmd: &str, p: Value) -> String {
    let (record, steps) = (s.drawing("test").ok().cloned(), s.undo.len());
    let message = s.execute(cmd, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"));
    assert_eq!((s.drawing("test").ok().cloned(), s.undo.len()), (record, steps), "{p}");
    message
}

/// A drawing of the block, with no views yet.
fn block_drawing() -> Session {
    let mut s = block();
    run(&mut s, "drawing.create", json!({}));
    s
}

/// A base view with no scale takes the largest preset at which it lies inside the sheet's drawing
/// space (not the paper), centred where it goes: the 100 x 20 front view on A3
/// at 2:1, as the snapshot fits it; that scale is saved as the view's own. A scale given is kept,
/// the settings given are the view's own, each view is one undo step ([`create`]), and with nothing
/// to measure the view takes 1:1.
#[test]
fn a_base_view_fits_its_sheet_and_saves_the_scale() {
    let mut s = block_drawing();
    let source = s.drawing("test").unwrap().sources[0].id;
    let (made, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1"}));
    assert_eq!((made["scale"]["text"].as_str(), made["scale"]["fitted"].as_bool()), (Some("2:1"), Some(true)));
    assert_box(&made, [110.0, 128.5], [310.0, 168.5]);
    assert!(made.get("outside").is_none(), "{made}");
    let view = saved(&s, front);
    assert_eq!((view.kind, view.place), (ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) }, ViewPlace::At([210.0, 148.5])));
    assert_eq!(view.settings, ViewOverrides { scale: Some("2:1".parse().unwrap()), ..ViewOverrides::default() }, "the fitted scale is saved");
    assert_eq!(inspect_view(&mut s, front)["settings"]["scale"], json!({"value": "2:1", "from": "this", "set": "2:1"}));
    let near_the_left = create(&mut s, BASE, json!({"sheet": "Sheet 1", "position_mm": [108, 148.5]})).0;
    assert_eq!(near_the_left["scale"]["text"], "1:1", "98 mm to the drawing border: 196 mm of room (the paper's edge would give 216)");
    let p = json!({"sheet": "Sheet 1", "view": "top", "scale": "1:5", "position_mm": [100, 100], "visual_style": "visible_hidden", "reference_shown_percent": 50});
    let (given, top) = create(&mut s, BASE, p);
    assert_eq!((given["scale"]["text"].as_str(), given["scale"]["fitted"].as_bool()), (Some("1:5"), Some(false)));
    assert_box(&given, [90.0, 95.0], [110.0, 105.0]);
    let view = saved(&s, top);
    assert_eq!(
        view.settings,
        ViewOverrides { scale: Some("1:5".parse().unwrap()), visual_style: Some(VisualStyle::VisibleHidden), ..ViewOverrides::default() }
    );
    assert_eq!(view.reference_shown_percent, 50.0, "it has no reference parts, so its box doesn't grow");
    let before = s.drawing("test").unwrap().views.len();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.drawing("test").unwrap().views.len(), before - 1, "one undo takes the view away");
    assert!(s.drawing("test").unwrap().views.iter().all(|v| v.id != top));
    // Nothing to measure: one sketch, something to draw (an empty design isn't), and no bodies.
    let mut sketch_only = Session::default();
    run(&mut sketch_only, "sketch.create", json!({"plane": "XY"}));
    run(&mut sketch_only, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 5]}));
    run(&mut sketch_only, "sketch.finish", json!({}));
    run(&mut sketch_only, "drawing.create", json!({}));
    let (placeholder, _) = create(&mut sketch_only, BASE, json!({"sheet": "Sheet 1"}));
    assert_eq!((placeholder["scale"]["text"].as_str(), placeholder["scale"]["fitted"].as_bool()), (Some("1:1"), Some(false)));
    assert_box(&placeholder, [200.0, 138.5], [220.0, 158.5]);
}

/// A projected view is lined up with its parent, its boundary 20 mm past the parent's on its side,
/// and follows its parent's scale and settings: right of the 50 x 10 front view at 1:2 (185..235 x
/// 143.5..153.5) the 25 x 10 right view goes to 255..280, left of it to 140..165, the 50 x 25 views
/// above and below to 173.5..198.5 and 98.5..123.5. The view above shows the top in third angle and
/// the bottom in first. A place given is kept; one past the drawing space is listed, not moved.
#[test]
fn a_projected_view_lines_up_a_gap_past_its_parent() {
    let mut s = block_drawing();
    let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": "1:2"}));
    let project = |s: &mut Session, side: &str| create(s, PROJECTED, json!({"parent": ids::encode(front), "side": side}));
    let (right, right_id) = project(&mut s, "right");
    assert_box(&right, [255.0, 143.5], [280.0, 153.5]);
    assert_eq!(right["scale"]["text"], "1:2");
    assert_eq!(right["sheet"], json!(ids::encode(saved(&s, front).sheet)));
    let view = saved(&s, right_id);
    assert_eq!(
        (view.kind, view.place, view.settings),
        (ViewKind::Projected { parent: front, side: ProjectedSide::Right }, ViewPlace::Aligned(267.5), ViewOverrides::default())
    );
    let d = s.drawing("test").unwrap().clone();
    assert!((d.view_centre(right_id).unwrap() - Vec2::new(267.5, 148.5)).len() < EPS, "on the parent's line");
    assert_box(&project(&mut s, "left").0, [140.0, 143.5], [165.0, 153.5]);
    let (above, above_id) = project(&mut s, "above");
    assert_box(&above, [185.0, 173.5], [235.0, 198.5]);
    assert_box(&project(&mut s, "below").0, [185.0, 98.5], [235.0, 123.5]);
    assert_eq!(inspect_view(&mut s, above_id)["direction"]["named"], "top", "third angle");
    set_projection(&mut s, Projection::First);
    assert_eq!(inspect_view(&mut s, above_id)["direction"]["named"], "bottom", "first angle");
    let at = json!({"parent": ids::encode(front), "side": "right", "place": {"at": [300.0, 60.0]}});
    let (_, apart) = create(&mut s, PROJECTED, at);
    assert_eq!(saved(&s, apart).place, ViewPlace::At([300.0, 60.0]));
    let past = json!({"parent": ids::encode(front), "side": "right", "place": {"aligned": 405.0}});
    let (beyond, beyond_id) = create(&mut s, PROJECTED, past);
    assert_eq!(outside(&beyond).1, [beyond_id], "listed: {beyond}");
    assert_eq!(saved(&s, beyond_id).place, ViewPlace::Aligned(405.0), "not moved");
    // A projected view of a projected view lines up with its own parent.
    let (_, back) = create(&mut s, PROJECTED, json!({"parent": ids::encode(right_id), "side": "right"}));
    assert_eq!(saved(&s, back).place, ViewPlace::Aligned(267.5 + 12.5 + 20.0 + 25.0));
    // On its parent's sheet, whichever that is.
    let second = second_sheet(&mut s);
    let (_, other) = create(&mut s, BASE, json!({"sheet": ids::encode(second), "scale": "1:2"}));
    let (beside, beside_id) = create(&mut s, PROJECTED, json!({"parent": ids::encode(other), "side": "below"}));
    assert_eq!((beside["sheet"].clone(), saved(&s, beside_id).sheet), (json!(ids::encode(second)), second));
}

/// A base view with no scale fits what its percent shows, the scale the snapshot gives for the same
/// percent on a sheet with no block, as a snapshot has none, and stays inside the drawing space on the
/// side the box grows on.
#[test]
fn a_base_view_fits_what_its_percent_shows() {
    let (mut s, [_, block]) = plate_and_block(5.0);
    run(&mut s, "occurrence.move", json!({"occurrence": block, "translate": [3000, 10, 5]}));
    let snapshot = |s: &mut Session, percent: f64| snap(s, json!({"view": "top", "reference_shown_percent": percent}))["scale"]["text"].clone();
    let (near, far) = (snapshot(&mut s, 0.0), snapshot(&mut s, 100.0));
    assert_ne!(near, far);
    // On the snapshot's A3, with no block, as a snapshot has none.
    run(&mut s, "drawing.create", json!({"template": "iso-a3-assembly"}));
    run(&mut s, "drawing.sheet.set", json!({"sheet": "Sheet 1", "title_block": null}));
    for (percent, scale) in [(0.0, near), (100.0, far)] {
        let (made, id) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "view": "top", "reference_shown_percent": percent}));
        assert_eq!((&made["scale"]["text"], made["scale"]["fitted"].as_bool()), (&scale, Some(true)), "{made}");
        assert!(made.get("outside").is_none(), "{made}");
        assert_eq!(saved(&s, id).reference_shown_percent, percent);
    }
}

/// A 100 mm square view at the A3 sheet's middle would be 2:1 (200 mm across, 277 mm of room).
/// A block in any corner reaching into that box brings it to 1:1, one whose side only meets it leaves
/// it, one under the middle fits nothing, whatever preset, and no block is what the border alone says.
#[test]
fn a_fitted_view_keeps_off_a_block_in_any_corner() {
    let model = Aabb2 { min: Vec2::ZERO, max: Vec2::new(100.0, 100.0) };
    let extents = Extents { model, error_mm: 0.0, bodies: 1 };
    let space = Aabb2 { min: Vec2::new(20.0, 10.0), max: Vec2::new(410.0, 287.0) };
    let middle = Vec2::new(210.0, 148.5);
    let scaled = |block: Option<Aabb2>| fit(&extents, model, middle, Room { space, block }, BASE).map(|scale| scale.to_string());
    assert_eq!(scaled(None).unwrap(), "2:1");
    let at = |x0: f64, y0: f64, x1: f64, y1: f64| Some(Aabb2 { min: Vec2::new(x0, y0), max: Vec2::new(x1, y1) });
    // The box at 2:1 is 110 to 310 across and 48.5 to 248.5 up.
    let corners = [at(230.0, 10.0, 410.0, 59.0), at(20.0, 10.0, 200.0, 59.0), at(230.0, 238.0, 410.0, 287.0), at(20.0, 238.0, 200.0, 287.0)];
    for block in corners {
        assert_eq!(scaled(block).unwrap(), "1:1", "{block:?}");
    }
    assert_eq!(scaled(at(230.0, 10.0, 410.0, 48.5)).unwrap(), "2:1", "a side on the box's is clear");
    assert_eq!(scaled(at(310.0, 10.0, 410.0, 287.0)).unwrap(), "2:1", "a side on the box's is clear");
    // The drawing space's whole, and past it: nothing is clear of them.
    for block in [at(20.0, 10.0, 410.0, 287.0), at(0.0, 0.0, 420.0, 297.0), at(200.0, 140.0, 220.0, 150.0)] {
        let refusal = scaled(block).unwrap_err().to_string();
        assert!(refusal.contains("off its title block, at no preset scale from 50:1 to 1:10000"), "{refusal}");
    }
    // A view of a point has no box to keep off anything clear of the middle.
    let point = Aabb2 { min: Vec2::ZERO, max: Vec2::ZERO };
    let dot = Extents { model: point, ..extents };
    assert_eq!(fit(&dot, point, middle, Room { space, block: corners[0] }, BASE).unwrap().to_string(), "50:1");
}

/// With no block the fit is what the border alone gives, scale for scale, at every size and place.
#[test]
fn a_fit_with_no_block_is_the_borders() {
    let space = Aabb2 { min: Vec2::new(20.0, 10.0), max: Vec2::new(410.0, 287.0) };
    for (w, h, x, y) in
        [(100.0, 20.0, 210.0, 148.5), (1e4, 3.0, 40.0, 20.0), (0.01, 0.01, 409.0, 286.0), (400.0, 300.0, 210.0, 148.5), (10.0, 10.0, 5.0, 148.5)]
    {
        let model = Aabb2 { min: Vec2::ZERO, max: Vec2::new(w, h) };
        let extents = Extents { model, error_mm: 0.0, bodies: 1 };
        let at = Vec2::new(x, y);
        let got = fit(&extents, model, at, Room { space, block: None }, BASE).ok();
        let within = Vec2::new(2.0 * (at.x - space.min.x).min(space.max.x - at.x), 2.0 * (at.y - space.min.y).min(space.max.y - at.y));
        assert_eq!(got, Scale::fit(model.size(), within), "{w} x {h} at {at:?}");
    }
}

/// The base view on a real sheet, its built-in block 180 x 61 mm on the border's lower-right corner
/// (230 to 410 across, 10 to 71 up): at 2:1 the 100 x 20 view at (210, 82) would reach down to 62, so
/// it is 1:1, and listed as nothing. Over the block no scale will do, and that says so; with a scale
/// given it goes there and is listed, as ever. A sheet with no block fits as the border says, however
/// small, and a block the paper can't hold is refused where it's set.
#[test]
fn a_base_view_fits_around_the_sheets_block() {
    let mut s = block_drawing();
    run(&mut s, "drawing.sheet.set", json!({"sheet": "Sheet 1", "title_block": "built-in"}));
    let (off, _) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "position_mm": [210, 82]}));
    assert_eq!(off["scale"]["text"], "1:1", "{off}");
    assert_box(&off, [160.0, 72.0], [260.0, 92.0]);
    assert!(off.get("outside").is_none(), "{off}");
    let (roomy, _) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "position_mm": [210, 148.5]}));
    assert_eq!(roomy["scale"]["text"], "2:1", "{roomy}");
    let refusal = create_refused(&mut s, BASE, json!({"sheet": "Sheet 1", "position_mm": [320, 35]}));
    assert!(refusal.contains("off its title block, at no preset scale"), "{refusal}");
    let (given, over) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "position_mm": [320, 35], "scale": "1:1"}));
    assert_eq!(outside(&given).1, [over]);

    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let bare = |s: &mut Session, sides: [f64; 2], at: [f64; 2]| {
        sheet_set(s, sheet, json!({"size": {"custom": {"sides_mm": sides}}, "title_block": null}));
        s.execute(BASE, &json!({"sheet": "Sheet 1", "position_mm": at}))
    };
    let free = bare(&mut s, [420.0, 297.0], [320.0, 35.0]).unwrap();
    assert_eq!(free["scale"]["text"], "1:1", "the border's room: 180 x 50 mm: {free}");
    let tiny = bare(&mut s, [45.0, 30.0], [22.5, 15.0]).unwrap();
    assert_eq!(tiny["scale"]["text"], "1:5", "25 x 10 mm of room: {tiny}");
    let past = bare(&mut s, [45.0, 30.0], [0.0, 15.0]).unwrap_err().to_string();
    assert!(past.contains("drawing area at no preset scale"), "{past}");
    let mut big = solidcraft_geom::title_block::default_block();
    big.size_mm = Vec2::new(5000.0, 49.0);
    let refused = s.execute("drawing.sheet.set", &json!({"sheet": "Sheet 1", "title_block": big})).unwrap_err().to_string();
    assert!(refused.contains("a title block"), "{refused}");
}

/// What view creation refuses, each with the drawing and the undo steps unchanged: no drawing, a
/// drawing that draws no design, unknown or badly given sheets, parents and sides, places and
/// percents past their limits, a scale that doesn't fit, unknown fields, and a 33rd view in a chain.
#[test]
fn hostile_view_creation_is_refused() {
    let mut s = block();
    assert!(create_refused(&mut s, BASE, json!({"sheet": "Sheet 1"})).contains("drawing"));
    let mut s = block_drawing();
    let base = |p: Value| {
        let mut p = p;
        p["sheet"] = p.get("sheet").cloned().unwrap_or(json!("Sheet 1"));
        p
    };
    for p in [
        json!({"sheet": "Sheet 9"}),
        json!({"sheet": 1}),
        json!({"sheet": null}),
        base(json!({"view": "sideways"})),
        base(json!({"scale": "0:1"})),
        base(json!({"scale": "1:1", "position_mm": [1e300, 0]})),
        base(json!({"position_mm": [1e300, 0]})),
        base(json!({"position_mm": [5, 148.5]})),
        base(json!({"reference_shown_percent": 100.001})),
        base(json!({"reference_shown_percent": -1})),
        base(json!({"visual_style": "glossy"})),
        base(json!({"colour": "red"})),
        base(json!({"position_mm": null})),
    ] {
        create_refused(&mut s, BASE, p);
    }
    assert!(create_refused(&mut s, BASE, json!({})).contains("sheet"));
    let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": "1:2"}));
    let parent = json!(ids::encode(front));
    for p in [
        json!({"parent": ids::encode(99), "side": "right"}),
        json!({"parent": 1, "side": "right"}),
        json!({"parent": parent, "side": "behind"}),
        json!({"parent": parent}),
        json!({"parent": parent, "side": "right", "place": {"aligned": 1e300}}),
        json!({"parent": parent, "side": "right", "place": {"at": [0, 1e300]}}),
        json!({"parent": parent, "side": "right", "place": null}),
        json!({"parent": parent, "side": "right", "scale": "1:1"}),
    ] {
        create_refused(&mut s, PROJECTED, p);
    }
    // A chain is at most 32 views: the base view and 31 projected from it, one from the next.
    let mut last = front;
    for _ in 1..32 {
        last = create(&mut s, PROJECTED, json!({"parent": ids::encode(last), "side": "above", "place": {"at": [100.0, 100.0]}})).1;
    }
    let message = create_refused(&mut s, PROJECTED, json!({"parent": ids::encode(last), "side": "above"}));
    assert!(message.contains("at most 32"), "{message}");
    // A drawing that draws no design.
    let mut d = (**s.drawing("test").unwrap()).clone();
    d.sources.clear();
    d.views.clear();
    s.doc_mut().put_drawing(Arc::new(d));
    assert!(create_refused(&mut s, BASE, json!({"sheet": "Sheet 1"})).contains("draws no design"));
}

/// A scale typed with spaces or tabs round its numbers is the same scale, still written without
/// them: through the parser, a file's reader, `drawing.view.base` and `drawing.view.edit`.
#[test]
fn a_scale_typed_with_spaces_round_its_numbers_is_the_same_scale() {
    for (typed, plain) in [("1 : 1", "1:1"), (" 1 :2 ", "1:2"), ("1\t:\t2", "1:2"), ("2.5\u{a0}:\u{a0}1", "2.5:1")] {
        let plain: Scale = plain.parse().unwrap();
        assert_eq!(typed.parse::<Scale>().ok(), Some(plain), "{typed:?}");
        assert_eq!(serde_json::from_value::<Scale>(json!(typed)).ok(), Some(plain), "{typed:?}");
        assert_eq!(typed.parse::<Scale>().unwrap().to_string(), plain.to_string(), "{typed:?}");
    }
    let mut s = block_drawing();
    let (made, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": " 1 :2 "}));
    assert_eq!(made["scale"]["text"], "1:2");
    assert_eq!(saved(&s, front).settings.scale, Some("1:2".parse().unwrap()));
    for (typed, plain) in [("1 : 1", "1:1"), ("1\t:\t5", "1:5")] {
        edit_view(&mut s, front, json!({"scale": typed}));
        assert_eq!(saved(&s, front).settings.scale, Some(plain.parse().unwrap()), "{typed:?}");
        assert_eq!(inspect_view(&mut s, front)["settings"]["scale"]["value"], plain, "{typed:?}");
    }
}

/// Seeded runs of spaces, tabs and no-break spaces round the numbers of scales, valid and hostile,
/// give the answer the text gets without them, from the parser and a file's reader alike. A space
/// inside a number, a text with no colon and the ratio sign stay refused, and nothing panics.
#[test]
fn seeded_spaces_round_a_scales_numbers_change_nothing() {
    let pick = |seed: u64, salt: u64, n: usize| ((seed ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) as usize % n;
    // Up to three of a space, a tab and a no-break space (U+00A0): `trim` removes each.
    let blank =
        |seed: u64, salt: u64| (0..pick(seed, salt, 4) as u64).map(|i| [' ', '\t', '\u{a0}'][pick(seed, salt * 8 + i, 3)]).collect::<String>();
    let huge = "9".repeat(400);
    // Each side is a plain number for 3 in 4 values of two bits of the seed, so exactly 9 in 16 texts
    // have two, and any two plain numbers make a scale (their ratios stay within 1:400 and 400:1).
    let plain = ["1", "2", "2.5", "0.5", "10", "0.25", "7", "100"];
    let other = ["1000", "100000", "0.000001", "1000001", "0", "", "-1", "+1", "1e3", "NaN", "inf", "1.", ".5", "1..5", ":2", huge.as_str()];
    let part = |seed: u64, side: u64| {
        let (list, salt) = if !(seed >> (2 * side)).is_multiple_of(4) { (&plain[..], 1) } else { (&other[..], 7) };
        list[pick(seed, salt + side, list.len())]
    };
    let read = |text: &str| (text.parse::<Scale>().ok(), serde_json::from_value::<Scale>(json!(text)).ok());
    let mut scales = 0;
    for seed in 1..=2000u64 {
        let (paper, model) = (part(seed, 0), part(seed, 1));
        let typed = format!("{}{paper}{}:{}{model}{}", blank(seed, 3), blank(seed, 4), blank(seed, 5), blank(seed, 6));
        let plain = read(&format!("{paper}:{model}"));
        assert_eq!(read(&typed), plain, "seed {seed}: {typed:?}");
        scales += usize::from(plain.0.is_some());
    }
    assert!(scales >= 1125, "{scales} of the 2000 texts were scales: at least the 1125 (9 in 16) of two plain numbers are");
    let refused =
        ["1 1", ":", "1:", " : ", "1::2", "-1 : 2", "1e3:1", "0:1", "NaN:1", "inf:1", "", "1 0:1", "1:2 0", "2 .5:1", "1\u{2236}1", "1 \u{2236} 1"];
    for text in refused {
        assert_eq!(read(text), (None, None), "{text:?}");
    }
}

/// A view's ids come from the session's id source: sessions seeded alike make the same ids.
#[test]
fn a_new_views_id_comes_from_the_sessions_seed() {
    let made = |seed: u64| {
        let mut s = block_drawing();
        s.seed_ids(seed);
        let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1"}));
        [front, create(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": "right"})).1]
    };
    assert_eq!(made(7), made(7));
    assert_ne!(made(7), made(8));
}

/// The block's boundary in each view at 1:2, worked by hand: front 50 x 10, top 50 x 25, right
/// 25 x 10, and from an isometric corner (100 + 50)/sqrt(2)/2 by (100 + 50 + 2 x 20)/sqrt(6)/2.
/// The centre lands on the position, and the extents are the model's own millimetres.
#[test]
fn a_blocks_boundary_in_each_view_is_its_size_at_the_scale() {
    let mut s = block();
    let views = [
        (json!("front"), (50.0, 10.0)),
        (json!("top"), (50.0, 25.0)),
        (json!("right"), (25.0, 10.0)),
        (json!("iso"), (150.0 / 2f64.sqrt() / 2.0, 190.0 / 6f64.sqrt() / 2.0)),
        (json!({"direction": [0, 1, 0], "up": [0, 0, 1]}), (50.0, 10.0)),
    ];
    for (view, (w, h)) in views {
        let v = snap(&mut s, json!({"view": view, "scale": "1:2", "position_mm": [100, 120]}));
        let (size, centre) = boundary(&v);
        assert!((size - Vec2::new(w, h)).len() < EPS, "{view}: {size:?}");
        assert!((centre - Vec2::new(100.0, 120.0)).len() < EPS, "{view}: {centre:?}");
        assert_eq!((v["scale"]["text"].as_str(), v["scale"]["fitted"].as_bool()), (Some("1:2"), Some(false)));
    }
    let front = snap(&mut s, json!({"scale": "1:2"}));
    assert_eq!(front["view"]["named"], "front");
    assert_eq!((front["view"]["direction"].clone(), front["view"]["x"].clone()), (json!([0.0, 1.0, 0.0]), json!([1.0, 0.0, 0.0])));
    assert!((point(&front["extents"]["min"]) - Vec2::ZERO).len() < EPS && (point(&front["extents"]["max"]) - Vec2::new(100.0, 20.0)).len() < EPS);
    assert_eq!((front["extents"]["status"].as_str(), front["source"]["bodies"].as_u64()), (Some("approximate"), Some(1)));
    assert_eq!(front["source"]["timeline_at_end"], true);
    let iso = snap(&mut s, json!({"view": "iso", "scale": "1:2"}));
    assert_eq!(iso["view"]["named"], "iso_front_right");
}

/// With no scale the view takes the largest preset scale that fits the sheet's drawing border,
/// centred on the sheet: the 100 x 20 front view on A3 landscape at 2:1 (5:1 would be 500 mm
/// wide), boundary 110..310 x 128.5..168.5. A scale given is used as it is, even when it overhangs.
#[test]
fn without_a_scale_the_view_fits_and_centres_on_its_sheet() {
    let mut s = block();
    let v = snap(&mut s, json!({}));
    assert_eq!((v["scale"]["text"].as_str(), v["scale"]["fitted"].as_bool()), (Some("2:1"), Some(true)));
    assert!((point(&v["boundary_mm"]["min"]) - Vec2::new(110.0, 128.5)).len() < EPS, "{v}");
    assert!((point(&v["boundary_mm"]["max"]) - Vec2::new(310.0, 168.5)).len() < EPS, "{v}");
    let a4 = snap(&mut s, json!({"sheet": {"size": {"standard": "a4"}, "orientation": "portrait"}}));
    assert_eq!(a4["scale"]["text"], "1:1", "A4 portrait has 95 mm to the border each side: 190 mm of room, so 2:1 (200 mm) doesn't fit");
    let near_the_left = snap(&mut s, json!({"position_mm": [80, 148.5]}));
    assert_eq!(near_the_left["scale"]["text"], "1:1", "70 mm to the left border: 140 mm of room");
    let overhanging = snap(&mut s, json!({"scale": "5:1"}));
    assert!((boundary(&overhanging).0 - Vec2::new(500.0, 100.0)).len() < EPS);
    let off_the_border = refused(&mut s, json!({"position_mm": [5, 148.5]}));
    assert!(off_the_border.contains("no preset scale from 50:1 to 1:10000"), "{off_the_border}");
    assert_eq!(a4["sheet"], json!({"size": {"standard": "a4"}, "orientation": "portrait", "dpi": 300.0}), "the sheet, its defaults filled in");
}

/// Scale and position only scale and move the boundary; the browser's eye counts for nothing;
/// nothing in the design changes.
#[test]
fn scale_and_position_set_the_boundary_and_the_design_is_untouched() {
    let mut s = block();
    let (undo, doc) = (s.undo.len(), s.doc.clone());
    let dirty = s.is_dirty();
    let one = boundary(&snap(&mut s, json!({"scale": "1:1", "position_mm": [0, 0]})));
    let half = boundary(&snap(&mut s, json!({"scale": "1:2", "position_mm": [0, 0]})));
    assert!((one.0 - half.0 * 2.0).len() < EPS && one.1.len() < EPS && half.1.len() < EPS);
    // The same call gives the same answer, apart from what working it out took.
    let answer = |mut v: Value| {
        if let Some(reply) = v.as_object_mut() {
            reply.remove("stats");
        }
        v
    };
    let first = answer(snap(&mut s, json!({"view": "top"})));
    assert_eq!(answer(snap(&mut s, json!({"view": "top"}))), first, "the same call gives the same answer");
    assert_eq!((s.undo.len(), s.is_dirty()), (undo, dirty));
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc), "nothing in the design changes");
    run(&mut s, "browser.visibility", json!({"items": ["Block"], "visible": false}));
    assert_eq!(snap(&mut s, json!({"view": "top"}))["boundary_mm"], first["boundary_mm"], "a hidden body is still drawn");
}

/// A shaded view has a picture: the block's front view at 1:2 is 50 x 10 mm, 591 x 119 px at
/// 300 dpi (rounded outward) and 2363 x 473 at 1200, and at 1200 too on a 2400 dpi sheet (lines may
/// export finer, a picture is drawn at most at 1200); a view with nothing shaded in it has none. A
/// shaded part's own lines are left out, its picture draws it: the block's, a model part's, when
/// the model is shaded, not when only reference parts are.
#[test]
fn a_shaded_view_has_the_picture_of_its_boundary() {
    let mut s = block();
    let reply = snap(&mut s, json!({"visual_style": "shaded", "scale": "1:2"}));
    let schema = output_schema_for::<SnapshotResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!(reply["lines"], json!([]), "the block is shaded: its picture draws it");
    assert_eq!(reply["picture"]["pixels"], json!([591, 119]));
    assert_eq!(reply["picture"]["dpi"], 300.0);
    assert_eq!(reply["picture"]["rect_mm"], reply["boundary_mm"]);
    let fine = snap(&mut s, json!({"visual_style": "shaded", "scale": "1:2", "sheet": {"dpi": 1200}}));
    assert_eq!(fine["picture"]["pixels"], json!([2363, 473]));
    let finest = snap(&mut s, json!({"visual_style": "shaded", "scale": "1:2", "sheet": {"dpi": 2400}}));
    assert_eq!(
        (&finest["picture"]["pixels"], &finest["picture"]["dpi"], &finest["sheet"]["dpi"]),
        (&json!([2363, 473]), &json!(1200.0), &json!(2400.0))
    );
    assert!(snap(&mut s, json!({"scale": "1:2"})).get("picture").is_none(), "lines only");
    let none = snap(&mut s, json!({"reference_style": "shaded", "scale": "1:2"}));
    assert!(none.get("picture").is_none(), "shaded reference parts, and none in the design: {none}");
    assert_eq!(none["lines"], snap(&mut s, json!({"scale": "1:2"}))["lines"], "the block's lines, as a model part's");
}

/// A shaded part's lines are left out and the other style's parts keep theirs: a plate, and a hinge
/// holding a pin placed twice, the hinge a reference component. A shaded model leaves only the
/// pins' lines; shaded reference parts leave only the plate's; between them, every body's.
#[test]
fn a_shaded_part_leaves_out_only_its_own_lines() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 3}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.create", json!({"name": "Hinge", "activate": false}));
    run(&mut s, "component.create", json!({"name": "Pin", "parent": "Hinge"}));
    run(&mut s, "solid.cylinder", json!({"diameter": 4, "height": 30}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.copy", json!({"component": "Hinge", "translate": [0, 50, 0]}));
    run(&mut s, "component.metadata.set", json!({"component": "Hinge", "bom_type": "reference"}));
    let references = |reply: &Value| -> Vec<bool> { reply["lines"].as_array().unwrap().iter().map(|l| l["reference"].as_bool().unwrap()).collect() };
    let bodies = |reply: &Value| -> std::collections::BTreeSet<String> {
        reply["lines"].as_array().unwrap().iter().map(|l| l["body"].as_str().unwrap().to_owned()).collect()
    };
    let all = snap(&mut s, json!({}));
    let model_shaded = snap(&mut s, json!({"visual_style": "shaded"}));
    let references_shaded = snap(&mut s, json!({"reference_style": "shaded"}));
    assert!(!references(&model_shaded).is_empty() && references(&model_shaded).iter().all(|r| *r), "{model_shaded}");
    assert!(!references(&references_shaded).is_empty() && references(&references_shaded).iter().all(|r| !*r), "{references_shaded}");
    assert_eq!(bodies(&model_shaded).union(&bodies(&references_shaded)).cloned().collect::<std::collections::BTreeSet<_>>(), bodies(&all));
}

/// What the snapshot refuses, each with nothing changed: bad settings, the shaded styles with
/// lines until the view's exact lines, the sheet's resolution out of range, an empty design, a
/// position off the sheet's limit, and a view that would reach past the paper's coordinates.
#[test]
fn hostile_snapshots_are_refused() {
    let mut s = block();
    let doc = s.doc.clone();
    let cases = [
        (json!({"scale": "0:1"}), "scale"),
        (json!({"scale": "-1:2"}), "scale"),
        (json!({"scale": "1e400:1"}), "scale"),
        (json!({"scale": "1:2:3"}), "scale"),
        (json!({"scale": 2}), "scale"),
        (json!({"scale": null}), "scale"),
        (json!({"view": "Front"}), "view is"),
        (json!({"view": {"direction": [0, 0, 0], "up": [0, 0, 1]}}), "direction"),
        (json!({"view": {"direction": [1e308, 1e308, 0], "up": [0, 0, 1]}}), "direction"),
        (json!({"view": {"direction": [0, 0, 1], "up": [0, 0, 2]}}), "along the view direction"),
        (json!({"visual_style": "Shaded"}), "unknown variant"),
        (json!({"tangent_edges": "thin"}), "thin"),
        (json!({"sheet": {"dpi": 24.9}}), "dpi"),
        (json!({"sheet": {"dpi": 2400.1}}), "dpi"),
        (json!({"sheet": {"size": {"custom": {"sides_mm": [0, 10]}}}}), "sheet"),
        (json!({"reference_shown_percent": -1}), "reference_shown_percent"),
        (json!({"position_mm": [1e308, 0]}), "point"),
        (json!({"position_mm": [5000.1, 0], "scale": "1:1"}), "position"),
        (json!({"scael": "1:2"}), "scael"),
    ];
    for (p, says) in cases {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(says), "{p}: {e}");
        assert!(std::sync::Arc::ptr_eq(&doc, &s.doc), "{p}");
    }
    let mut big = Session::default();
    run(&mut big, "solid.box", json!({"length": 300, "width": 10, "height": 10}));
    let e = refused(&mut big, json!({"scale": "1000:1"}));
    assert!(e.contains("past ±100000 mm"), "{e}");
    let e = refused(&mut Session::default(), json!({}));
    assert!(e.contains("no bodies"), "{e}");
}

/// A view shows each placement of a part as a body of its own, named by the occurrences that place
/// it and its definition's body, and measures them all: the two cubes span 0 to 40 mm across.
#[test]
fn a_view_shows_each_placement_of_a_part_with_its_path() {
    let (mut s, [first, second]) = two_cubes();
    let set = crate::drawing_view::shown(&s).unwrap();
    let mut placed: Vec<(Vec<u64>, String)> = set.bodies.iter().map(|b| (b.path.clone(), b.body.clone())).collect();
    placed.sort();
    assert_eq!(placed, vec![(vec![first], "Cube".to_string()), (vec![second], "Cube".to_string())]);
    let v = snap(&mut s, json!({"scale": "1:1"}));
    assert_eq!(v["source"]["bodies"], 2, "{v}");
    let (min, max) = (point(&v["extents"]["min"]), point(&v["extents"]["max"]));
    assert!((max.x - min.x - 40.0).abs() < 1e-6 && (max.y - min.y - 10.0).abs() < 1e-6, "{v}");
}

/// Every straight segment of `v`'s pieces at `visibility`, on the sheet.
fn segments(v: &Value, visibility: &str) -> Vec<(Vec2, Vec2)> {
    let pieces = v["lines"].as_array().into_iter().flatten().flat_map(|l| l["pieces"].as_array().into_iter().flatten());
    pieces
        .filter(|p| p["visibility"] == visibility)
        .flat_map(|p| p["path"].as_array().into_iter().flatten())
        .filter(|seg| seg["type"] == "line")
        .map(|seg| (point(&seg["a"]), point(&seg["b"])))
        .collect()
}

/// Every arc of `v`'s pieces at `visibility`, on the sheet: its centre, radius and sweep, and its
/// piece's accuracy.
fn arcs(v: &Value, visibility: &str) -> Vec<(Vec2, f64, f64, Value)> {
    let pieces = v["lines"].as_array().into_iter().flatten().flat_map(|l| l["pieces"].as_array().into_iter().flatten());
    let mut out = Vec::new();
    for piece in pieces.filter(|p| p["visibility"] == visibility) {
        for seg in piece["path"].as_array().into_iter().flatten().filter(|seg| seg["type"] == "arc") {
            out.push((point(&seg["center"]), seg["radius"].as_f64().unwrap(), seg["sweep"].as_f64().unwrap(), piece["accuracy"]["kind"].clone()));
        }
    }
    out
}

/// A 40 x 40 x 5 mm plate from the origin with a 10 mm hole through it at (20, 20): seen from
/// above at 1:1, the hole is round the A3 sheet's centre (210, 148.5).
fn plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"diameter": 10, "height": 7, "base": [20, 20, -1], "operation": "cut", "targets": ["Plate"]}));
    s
}

/// How long `v`'s pieces at `visibility` are altogether, on the sheet.
fn drawn_length(v: &Value, visibility: &str) -> f64 {
    segments(v, visibility).iter().map(|(a, b)| a.dist(*b)).sum()
}

/// The block's front view at 1:2, centred on the A3 sheet at (210, 148.5): visible lines round its
/// 50 x 10 mm outline and nothing else. The back edges lie under the front ones and are counted as
/// covered; the edges seen end on draw nothing. The lines are the block's exact edges, so they say
/// analytic, and certify any accuracy.
#[test]
fn a_views_lines_are_the_blocks_outline() {
    let mut s = block();
    let v = snap(&mut s, json!({"scale": "1:2"}));
    let schema = output_schema_for::<SnapshotResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &v), "{v}");
    assert!((drawn_length(&v, "visible") - 120.0).abs() < 1e-6, "{v}");
    assert!(segments(&v, "hidden").is_empty(), "{v}");
    let on_outline = |p: Vec2| {
        let inside = (185.0 - 1e-6..=235.0 + 1e-6).contains(&p.x) && (143.5 - 1e-6..=153.5 + 1e-6).contains(&p.y);
        let on_side = [(p.x - 185.0), (p.x - 235.0), (p.y - 143.5), (p.y - 153.5)].iter().any(|d| d.abs() < 1e-6);
        inside && on_side
    };
    assert!(segments(&v, "visible").iter().all(|(a, b)| on_outline(*a) && on_outline(*b)), "{v}");
    for line in v["lines"].as_array().unwrap() {
        assert_eq!((&line["body"], &line["occurrence"], &line["reference"]), (&json!("Block"), &json!([]), &json!(false)), "{line}");
        assert!(line["pieces"].as_array().unwrap().iter().all(|p| p["accuracy"]["kind"] == "analytic"), "{line}");
    }
    assert_eq!((&v["status"]["lines"], &v["status"]["provenance"]), (&json!("exact"), &json!("hidden_lines")));
    assert!(v["status"]["covered_pieces"].as_u64().unwrap() >= 4, "{v}");
    assert_eq!((v["stats"]["shared_meshes"].as_u64(), v["stats"]["triangles"].as_u64()), (Some(0), Some(0)), "nothing is meshed: {v}");
    snap(&mut s, json!({"scale": "1:2", "max_error_mm": 0.0001}));
}

/// A view looking as the front view does, its plane through another point, draws the same lines on
/// paper: a view is centred on what it shows, wherever its plane's origin is.
#[test]
fn a_views_origin_moves_nothing_on_paper() {
    let mut s = block();
    let front = snap(&mut s, json!({"scale": "1:2", "visual_style": "visible_hidden"}));
    let looking = json!({"direction": [0, 1, 0], "up": [0, 0, 1], "origin": [30, 7, 5]});
    let moved = snap(&mut s, json!({"view": looking, "scale": "1:2", "visual_style": "visible_hidden"}));
    let ends = |v: &Value, visibility: &str| {
        let mut ends: Vec<[f64; 4]> = segments(v, visibility).into_iter().map(|(a, b)| [a.x, a.y, b.x, b.y]).collect();
        ends.sort_by(|p, q| p.partial_cmp(q).unwrap());
        ends
    };
    for visibility in ["visible", "hidden"] {
        let (a, b) = (ends(&front, visibility), ends(&moved, visibility));
        assert!(!a.is_empty() || visibility == "hidden", "{front}");
        assert_eq!(a.len(), b.len(), "{visibility}: {moved}");
        assert!(a.iter().zip(&b).all(|(p, q)| p.iter().zip(q).all(|(x, y)| (x - y).abs() < 1e-9)), "{visibility}: {a:?} {b:?}");
    }
}

/// The block in iso, hidden lines too: its three back edges are hidden over all their length, each
/// one straight analytic piece, as long on paper as the block is along it at 1:2 (100, 50 and 20
/// mm, foreshortened to sqrt(2/3) of it), and nothing else is hidden.
#[test]
fn an_iso_views_back_edges_are_hidden_end_to_end() {
    let mut s = block();
    let v = snap(&mut s, json!({"view": "iso", "scale": "1:2", "visual_style": "visible_hidden"}));
    let hidden: Vec<&Value> =
        v["lines"].as_array().unwrap().iter().filter(|l| l["pieces"].as_array().unwrap().iter().any(|p| p["visibility"] == "hidden")).collect();
    assert_eq!(hidden.len(), 3, "{v}");
    let mut lengths: Vec<f64> = Vec::new();
    for line in hidden {
        let [piece] = line["pieces"].as_array().unwrap().as_slice() else { panic!("one piece: {line}") };
        assert_eq!((&piece["visibility"], &piece["accuracy"]["kind"]), (&json!("hidden"), &json!("analytic")), "{line}");
        let [seg] = piece["path"].as_array().unwrap().as_slice() else { panic!("one segment: {line}") };
        lengths.push(point(&seg["a"]).dist(point(&seg["b"])));
    }
    lengths.sort_by(f64::total_cmp);
    let along = (2.0f64 / 3.0).sqrt() / 2.0;
    for (got, want) in lengths.iter().zip([20.0, 50.0, 100.0]) {
        assert!((got - want * along).abs() < 1e-9, "{lengths:?}: {v}");
    }
}

/// A cylinder in iso: its rims, circles seen at an angle, are chords within `chord_mm` on paper,
/// bounded by it; its sides are one exact silhouette line each, named by its first face.
#[test]
fn a_cylinder_in_iso_has_bounded_rims_and_exact_sides() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"diameter": 20, "height": 30, "body_name": "Cylinder"}));
    let v = snap(&mut s, json!({"view": "iso", "scale": "1:1"}));
    let lines = v["lines"].as_array().unwrap();
    let (rims, sides): (Vec<&Value>, Vec<&Value>) = lines.iter().partition(|l| l["source"]["kind"] == "edge");
    assert!(!rims.is_empty() && sides.len() == 2, "{v}");
    for piece in rims.iter().flat_map(|l| l["pieces"].as_array().unwrap()) {
        assert_eq!(piece["accuracy"], json!({"kind": "bounded", "max_deviation_mm": 0.01}), "{piece}");
        assert!(piece["path"].as_array().unwrap().iter().all(|seg| seg["type"] == "line"), "{piece}");
    }
    for side in sides {
        let [piece] = side["pieces"].as_array().unwrap().as_slice() else { panic!("{side}") };
        assert_eq!(piece["accuracy"]["kind"], "analytic", "{side}");
        let [seg] = piece["path"].as_array().unwrap().as_slice() else { panic!("{side}") };
        assert!((point(&seg["a"]).dist(point(&seg["b"])) - 30.0 * (2.0f64 / 3.0).sqrt()).abs() < 1e-9, "{side}");
    }
    let faces: Vec<&Value> = lines.iter().filter(|l| l["source"]["kind"] == "silhouette").map(|l| &l["source"]["face"]).collect();
    assert_eq!(faces[0], faces[1], "both sides are the silhouette of one surface: {v}");
    assert_eq!((&v["status"]["lines"], &v["status"]["provenance"]), (&json!("exact"), &json!("hidden_lines")));
}

/// A cylinder's sides meet its rims, seen from each upper corner, so its rims' seams fall at either
/// tangent point in one view or another: each side ends where a visible piece of its bottom rim ends
/// (to 1e-6), and both its ends lie on a rim (within `chord_mm`). With hidden lines, the bottom rim's
/// hidden half starts at a side's end too. Every view is checked before the test fails, so its
/// message lists each side that misses. Listed in tests/KNOWN_FAILING_TESTS until `hlr` splits a rim
/// where a side line touches it (#2455).
#[test]
fn a_cylinders_sides_meet_its_rims_from_every_corner() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 15, "height": 50, "base": [80, 20, 0], "axis": "Z", "body_name": "Cylinder"}));
    // The ends of each piece of a rim, and its segments, at `visibility`.
    let rims = |v: &Value, visibility: &str| {
        let pieces = v["lines"].as_array().unwrap().iter().filter(|l| l["source"]["kind"] == "edge").flat_map(|l| l["pieces"].as_array().unwrap());
        let paths: Vec<Vec<(Vec2, Vec2)>> = pieces
            .filter(|p| p["visibility"] == visibility)
            .map(|p| p["path"].as_array().unwrap().iter().map(|seg| (point(&seg["a"]), point(&seg["b"]))).collect())
            .collect();
        let ends: Vec<Vec2> = paths.iter().flat_map(|path| [path.first().unwrap().0, path.last().unwrap().1]).collect();
        (ends, paths.concat())
    };
    let off = |p: Vec2, (a, b): (Vec2, Vec2)| {
        let t = ((p - a).dot(b - a) / (b - a).len2()).clamp(0.0, 1.0);
        p.dist(a + (b - a) * t)
    };
    // The distance from a side's nearer end to the nearest of `at`.
    let gap = |(a, b): (Vec2, Vec2), at: &[Vec2]| [a, b].iter().flat_map(|e| at.iter().map(|q| q.dist(*e))).fold(f64::INFINITY, f64::min);
    let mut wrong = Vec::new();
    for (x, y) in [(-1, 1), (1, 1), (1, -1), (-1, -1)] {
        let looking = json!({"direction": [x, y, -1], "up": [0, 0, 1]});
        for style in ["visible", "visible_hidden"] {
            let v = snap(&mut s, json!({"view": looking, "scale": "1:2", "visual_style": style}));
            let sides: Vec<(Vec2, Vec2)> = v["lines"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|l| l["source"]["kind"] == "silhouette")
                .flat_map(|l| l["pieces"].as_array().unwrap().iter().flat_map(|p| p["path"].as_array().unwrap()))
                .map(|seg| (point(&seg["a"]), point(&seg["b"])))
                .collect();
            assert_eq!(sides.len(), 2, "({x}, {y}) {style}: {v}");
            let (ends, segs) = rims(&v, "visible");
            let (hidden_ends, _) = rims(&v, "hidden");
            for side in sides {
                let visible_gap = gap(side, &ends);
                let hidden_gap = if style == "visible" { 0.0 } else { gap(side, &hidden_ends) };
                let off_rims = [side.0, side.1].map(|e| segs.iter().map(|seg| off(e, *seg)).fold(f64::INFINITY, f64::min));
                if visible_gap >= 1e-6 || hidden_gap >= 1e-6 || off_rims.iter().any(|d| *d > 0.01 + 1e-9) {
                    wrong.push(format!("({x}, {y}) {style}, side at x {:.3}: rim end {visible_gap:.3e} mm away, hidden {hidden_gap:.3e}, off the rims {off_rims:.3?}", side.0.x));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// `s` with a 20 mm cube inserted as a mesh body (from an STL), its corner at `corner`.
fn with_mesh_cube(s: &mut Session, corner: [f64; 3], tag: &str) {
    let dir = std::env::temp_dir().join(format!("solidcraft-view-mesh-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stl = dir.join("cube.stl");
    let mut cube = Session::default();
    run(&mut cube, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": corner, "body_name": "Cube"}));
    run(&mut cube, "file.export", json!({"path": stl.to_string_lossy()}));
    run(s, "file.insert_mesh", json!({"path": stl.to_string_lossy()}));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A mesh body is drawn from its mesh and a B-rep body beside it exactly. Once the mesh body stands
/// in front of the block, the block may be hidden by it where `hlr` can't see, so it is drawn from
/// the mesh too, its hidden part hidden; asked to certify, the view says why.
#[test]
fn a_body_a_mesh_body_could_hide_is_drawn_from_the_mesh() {
    // The accuracies of the pieces of the block's lines (`true`) or the others'.
    let kinds = |v: &Value, of_block: bool| -> std::collections::BTreeSet<String> {
        let lines = v["lines"].as_array().unwrap().iter().filter(|l| (l["body"] == "Block") == of_block);
        lines.flat_map(|l| l["pieces"].as_array().unwrap().iter()).map(|p| p["accuracy"]["kind"].as_str().unwrap().to_string()).collect()
    };
    let only = |kind: &str| std::collections::BTreeSet::from([kind.to_string()]);
    let mut apart = block();
    with_mesh_cube(&mut apart, [150.0, 0.0, 0.0], "apart");
    let v = snap(&mut apart, json!({"scale": "1:2"}));
    assert_eq!(v["status"]["provenance"], "hidden_lines_and_mesh_projection", "{v}");
    assert_eq!((kinds(&v, true), kinds(&v, false)), (only("analytic"), only("sampled")), "{v}");
    // The cube 20 mm across in front of the block's bottom edge, from x 40 to 60: 10 mm of it hidden
    // at 1:2.
    let mut in_front = block();
    with_mesh_cube(&mut in_front, [40.0, -30.0, -10.0], "front");
    let v = snap(&mut in_front, json!({"scale": "1:2", "visual_style": "visible_hidden"}));
    assert_eq!((&v["status"]["lines"], &v["status"]["provenance"]), (&json!("approximate"), &json!("mesh_projection")), "{v}");
    assert!((drawn_length(&v, "hidden") - 10.0).abs() < 1e-6, "the block's bottom edge behind the cube: {v}");
    let e = refused(&mut in_front, json!({"scale": "1:2", "max_error_mm": 0.01}));
    assert!(e.contains("bodies drawn from the mesh: 1 for mesh bodies, 1 for a mesh body in front"), "{e}");
}

/// A sheet's three views prepare the block for exact lines once between them (#13218). A mesh body
/// beside it is drawn from its mesh in every view, not only in the view that met it first. Two
/// copies of a part and a pin of their own: two bodies prepared, each drawn exact in every view by
/// its own edges (the pin has 3, the cube 12).
#[test]
fn a_sheets_views_prepare_each_body_once() {
    // Per view: the bodies it prepared, and the triangles it drew from meshes.
    let views = |s: &Session| -> Vec<(usize, usize)> {
        let d = s.drawing("test").unwrap().clone();
        let scene = crate::drawing_scene::build(s, &d, d.sheets[0].id).unwrap();
        scene.views.iter().map(|v| v.stats.map_or((0, 0), |stats| (stats.prepared, stats.triangles))).collect()
    };
    let prepared = |views: &[(usize, usize)]| views.iter().map(|(prepared, _)| prepared).sum::<usize>();
    let (s, _) = three_views();
    let drawn = views(&s);
    assert_eq!((drawn.len(), prepared(&drawn)), (3, 1), "{drawn:?}");
    assert!(drawn.iter().all(|(_, triangles)| *triangles == 0), "nothing from the mesh: {drawn:?}");
    let mut mesh = block();
    with_mesh_cube(&mut mesh, [150.0, 0.0, 0.0], "sheet");
    let drawn = views(&three_views_of(mesh).0);
    assert!(prepared(&drawn) == 1 && drawn.iter().all(|(_, triangles)| *triangles > 0), "{drawn:?}");
    let (mut copies, _) = two_cubes();
    run(&mut copies, "solid.cylinder", json!({"diameter": 6, "height": 10, "base": [5, 40, 0], "body_name": "Pin", "operation": "new"}));
    let drawn = views(&three_views_of(copies).0);
    assert_eq!((drawn.len(), prepared(&drawn)), (3, 2), "{drawn:?}");
    assert!(drawn.iter().all(|(_, triangles)| *triangles == 0), "nothing from the mesh: {drawn:?}");
}

/// A mirrored copy of a plate with a hole is drawn from its own record, not the mesh: from the top,
/// its outline and rim are the original's mirrored about the plane. The original's pieces and both
/// rims are analytic. The copy's straight edges come from the kernel as splines (a mirror isn't a
/// rigid move, so SMLib transforms the body), which `hlr` samples; they lie within 1e-12 of the line.
#[test]
fn a_mirrored_body_is_drawn_exact() {
    let mut s = plate();
    run(&mut s, "solid.mirror", json!({"bodies": ["Plate"], "plane": "YZ"}));
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1"}));
    assert_eq!((&v["status"]["provenance"], &v["source"]["bodies"]), (&json!("hidden_lines"), &json!(2)), "{v}");
    for line in v["lines"].as_array().unwrap() {
        for piece in line["pieces"].as_array().unwrap() {
            let (accuracy, arc) = (&piece["accuracy"], piece["path"][0]["type"] == "arc");
            let sampled_by_the_copy = line["body"] != "Plate" && !arc && accuracy["max_sample_deviation_mm"].as_f64().is_some_and(|d| d < 1e-12);
            assert!(accuracy["kind"] == "analytic" || sampled_by_the_copy, "{line}");
        }
    }
    let rims = arcs(&v, "visible");
    assert_eq!(rims.len(), 2, "one whole rim each: {v}");
    let centres: Vec<f64> = rims.iter().map(|(c, _, _, _)| c.x).collect();
    assert!(
        (centres[0] + centres[1] - 2.0 * point(&v["boundary_mm"]["min"]).x.midpoint(point(&v["boundary_mm"]["max"]).x)).abs() < 1e-9,
        "{centres:?}: {v}"
    );
}

/// A cylinder split round its middle keeps its exact lines and draws only the split from the mesh,
/// as the viewport does: `hlr` draws no edge between two faces of one surface yet (#2455). From the
/// front at 1:1 its outline (two 30 mm sides and two 20 mm rims seen edge on) is analytic, and the
/// split is one sampled line 20 mm across, within the mesh's chords. Asked to certify, the view says
/// which edges came from the mesh.
#[test]
fn a_split_round_face_draws_only_its_split_from_the_mesh() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"diameter": 20, "height": 30, "body_name": "Cylinder"}));
    run(&mut s, "solid.split_face", json!({"body": "Cylinder", "faces": [[-10, 0, 15]], "plane": {"origin": [0, 0, 15], "normal": [0, 0, 1]}}));
    let v = snap(&mut s, json!({"view": "front", "scale": "1:1"}));
    assert_eq!(v["status"]["provenance"], "hidden_lines_and_mesh_projection", "{v}");
    // `hlr`'s pieces are analytic or bounded; only the mesh's are sampled.
    let sampled = |l: &&Value| l["pieces"].as_array().unwrap().iter().any(|p| p["accuracy"]["kind"] == "sampled");
    let (split, exact): (Vec<&Value>, Vec<&Value>) = v["lines"].as_array().unwrap().iter().partition(sampled);
    let [split] = split.as_slice() else { panic!("one line from the mesh: {v}") };
    assert_eq!(split["source"]["kind"], "edge", "{split}");
    let across = drawn_length(&json!({"lines": [split]}), "visible");
    assert!((across - 20.0).abs() < 0.05, "{across}: {split}");
    // The rest stays `hlr`'s: both side silhouettes and both rims.
    let kinds: Vec<&str> = exact.iter().filter_map(|l| l["source"]["kind"].as_str()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "silhouette").count(), 2, "{v}");
    assert_eq!(kinds.iter().filter(|k| **k == "edge").count(), 2, "{v}");
    let e = refused(&mut s, json!({"view": "front", "scale": "1:1", "max_error_mm": 0.01}));
    assert!(e.contains("edges drawn from the mesh: 1 for split faces"), "{e}");
}

/// A 20 x 20 x 40 tower behind the block, 40 to 60 mm across: in the front view at 1:1 its sides
/// show above the block (20 to 40 mm up) and are hidden behind it below, 2 x 20 mm of hidden line,
/// which only `visible_hidden` draws.
#[test]
fn hidden_lines_are_drawn_only_when_the_style_asks() {
    let mut s = block();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 40, "corner": [40, 60, 0], "body_name": "Tower", "operation": "new"}));
    let v = snap(&mut s, json!({"scale": "1:1", "visual_style": "visible_hidden"}));
    assert!((drawn_length(&v, "hidden") - 40.0).abs() < 1e-6, "{v}");
    let tower = v["lines"].as_array().unwrap().iter().filter(|l| l["body"] == "Tower").count();
    assert!(tower > 0, "{v}");
    let v = snap(&mut s, json!({"scale": "1:1"}));
    assert!(segments(&v, "hidden").is_empty(), "{v}");
}

/// The lines name the occurrence each copy of a part is placed by.
#[test]
fn each_copys_lines_name_its_occurrence() {
    let (mut s, [first, second]) = two_cubes();
    let v = snap(&mut s, json!({"scale": "1:1"}));
    let mut paths: Vec<Value> = v["lines"].as_array().unwrap().iter().map(|l| l["occurrence"].clone()).collect();
    paths.dedup();
    paths.sort_by_key(|p| p.to_string());
    let mut want = vec![json!([first]), json!([second])];
    want.sort_by_key(|p| p.to_string());
    assert_eq!(paths, want, "{v}");
}

/// A view cancelled while its lines are made says so in a drawing's words, never as a DXF export:
/// cancelled while the exact lines are worked out (the block), or while a mesh body's are (a mesh
/// cube).
#[test]
fn a_view_cancelled_while_its_lines_are_made_says_so_on_a_drawing() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct CancelAt(&'static str, AtomicBool);
    impl solidcraft_kernel::watch::Watch for CancelAt {
        fn cancelled(&self) -> bool {
            self.1.load(Ordering::Relaxed)
        }
        fn progress(&self, stage: &'static str, _done: usize, _total: usize) {
            if stage == self.0 {
                self.1.store(true, Ordering::Relaxed);
            }
        }
    }
    let mut mesh = Session::default();
    with_mesh_cube(&mut mesh, [0.0, 0.0, 0.0], "cancel");
    for (mut s, stage) in [(block(), "hidden_lines"), (mesh, "dxf_occlusion")] {
        let watch = Arc::new(CancelAt(stage, AtomicBool::new(false)));
        let e = solidcraft_kernel::watch::watching(watch, || s.execute(CMD, &json!({}))).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("drawing the view was cancelled") && !e.contains("DXF"), "{stage}: {e}");
    }
}

/// The mesh's chord is `chord_mm` on paper over the scale. Below 1:100 it passes 1 mm and is still
/// taken, while the view DXF keeps its own 1 mm limit; at 1000:1 the default is finer than the
/// mesh's 0.0001 mm floor and is refused with the chord that would do.
#[test]
fn the_meshs_chord_follows_the_scale() {
    let mut s = block();
    let v = snap(&mut s, json!({"scale": "1:200"}));
    assert!((v["status"]["model_chord_mm"].as_f64().unwrap() - 2.0).abs() < 1e-12, "{v}");
    assert!((v["status"]["chord_mm"].as_f64().unwrap() - 0.01).abs() < 1e-15, "{v}");
    let e = refused(&mut s, json!({"scale": "1000:1"}));
    assert!(e.contains("finer than 0.0001 mm") && e.contains("chord_mm 0.1"), "{e}");
    let v = snap(&mut s, json!({"scale": "1000:1", "chord_mm": 0.2}));
    assert!((v["status"]["model_chord_mm"].as_f64().unwrap() - 2e-4).abs() < 1e-15, "{v}");
    let dxf = s.execute("file.export_view_dxf", &json!({"camera": solidcraft_viewport::Camera::default(), "tolerance": 2})).unwrap_err().to_string();
    assert!(dxf.contains("tolerance must be 0.0001..1 mm"), "{dxf}");
}

/// A pocket 40 x 10 mm into the block's back face, 10 mm deep: from the front at 1:1 only the
/// outline shows, and `visible_hidden` adds the pocket's outline as hidden lines, a 40 x 10
/// rectangle 190 to 230 across and 143.5 to 153.5 up (its floor lies under its mouth, covered).
#[test]
fn a_pocket_behind_the_front_face_is_hidden() {
    let mut s = block();
    run(&mut s, "solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [30, 40, 5], "operation": "cut", "targets": ["Block"]}));
    let v = snap(&mut s, json!({"scale": "1:1"}));
    assert!(segments(&v, "hidden").is_empty() && (drawn_length(&v, "visible") - 240.0).abs() < 1e-6, "{v}");
    let v = snap(&mut s, json!({"scale": "1:1", "visual_style": "visible_hidden"}));
    assert!((drawn_length(&v, "hidden") - 100.0).abs() < 1e-6, "{v}");
    let on_pocket = |p: Vec2| {
        let inside = (190.0 - 1e-6..=230.0 + 1e-6).contains(&p.x) && (143.5 - 1e-6..=153.5 + 1e-6).contains(&p.y);
        inside && [p.x - 190.0, p.x - 230.0, p.y - 143.5, p.y - 153.5].iter().any(|d| d.abs() < 1e-6)
    };
    assert!(segments(&v, "hidden").iter().all(|(a, b)| on_pocket(*a) && on_pocket(*b)), "{v}");
}

/// Two blocks side by side, touching at x = 100: from the front at 1:2 their shared side, 222.5 mm
/// across the sheet, is drawn once, by the first block, and the second block's copy is covered.
#[test]
fn two_touching_blocks_draw_their_shared_side_once() {
    let mut s = block();
    run(&mut s, "solid.box", json!({"length": 50, "width": 50, "height": 20, "corner": [100, 0, 0], "body_name": "Second", "operation": "new"}));
    let v = snap(&mut s, json!({"scale": "1:2"}));
    assert!((drawn_length(&v, "visible") - 180.0).abs() < 1e-6, "{v}");
    let at_shared = |line: &&Value| {
        let segs = line["pieces"].as_array().into_iter().flatten().flat_map(|p| p["path"].as_array().into_iter().flatten());
        segs.into_iter().any(|seg| (point(&seg["a"]).x - 222.5).abs() < 1e-6 && (point(&seg["b"]).x - 222.5).abs() < 1e-6)
    };
    let owners: Vec<&Value> = v["lines"].as_array().unwrap().iter().filter(at_shared).map(|l| &l["body"]).collect();
    assert_eq!(owners, [&json!("Block")], "{v}");
    assert!(v["status"]["covered_pieces"].as_u64().unwrap() >= 1, "{v}");
}

/// Every segment of `v`'s pieces at `visibility`, lines and arcs alike, on the sheet.
fn drawn(v: &Value, visibility: &str) -> Vec<solidcore_record::Seg2> {
    let pieces = v["lines"].as_array().into_iter().flatten().flat_map(|l| l["pieces"].as_array().into_iter().flatten());
    let segs = pieces.filter(|p| p["visibility"] == visibility).flat_map(|p| p["path"].as_array().into_iter().flatten());
    segs.map(|seg| serde_json::from_value(seg.clone()).unwrap()).collect()
}

/// The points of `seg` furthest out: its ends, and for an arc each quarter-turn point it passes.
fn reach(seg: &solidcore_record::Seg2) -> Vec<Vec2> {
    let mut out = vec![seg.start(), seg.end()];
    if let solidcore_record::Seg2::Arc { center, radius, start, sweep } = *seg {
        let (lo, span) = if sweep >= 0.0 { (start, sweep) } else { (start + sweep, -sweep) };
        for k in 0..8 {
            let angle = f64::from(k) * std::f64::consts::FRAC_PI_2;
            if (angle - lo).rem_euclid(std::f64::consts::TAU) <= span {
                out.push(center + Vec2::from_angle(angle) * radius);
            }
        }
    }
    out
}

/// Whether `a` and `b` share a stretch longer than 1e-6 mm: collinear lines that overlap, or arcs
/// of one circle whose turns overlap.
fn share_a_stretch(a: &solidcore_record::Seg2, b: &solidcore_record::Seg2) -> bool {
    use solidcore_record::Seg2::{Arc, Line};
    use std::f64::consts::TAU;
    match (*a, *b) {
        (Line { a: p, b: q }, Line { a: c, b: d }) => {
            let Some(u) = (q - p).normalized() else { return false };
            let off = |r: Vec2| (r - p).cross(u).abs();
            let (t0, t1) = ((c - p).dot(u), (d - p).dot(u));
            off(c) < 1e-7 && off(d) < 1e-7 && t0.max(t1).min((q - p).len()) - t0.min(t1).max(0.0) > 1e-6
        }
        (Arc { center: c0, radius: r0, start: s0, sweep: w0 }, Arc { center: c1, radius: r1, start: s1, sweep: w1 }) => {
            let range = |s: f64, w: f64| if w >= 0.0 { (s.rem_euclid(TAU), w) } else { ((s + w).rem_euclid(TAU), -w) };
            let ((l0, n0), (l1, n1)) = (range(s0, w0), range(s1, w1));
            let overlap = [-TAU, 0.0, TAU].iter().map(|k| (l0 + n0).min(l1 + k + n1) - l0.max(l1 + k)).fold(0.0, f64::max);
            c0.dist(c1) < 1e-6 && (r0 - r1).abs() < 1e-6 && overlap * r0 > 1e-6
        }
        _ => false,
    }
}

/// Seeded views of the sample design at mixed views, scales, chords, styles and tangent settings:
/// nothing panics, every piece stays inside the view's boundary (to within the chord and the
/// extents' own error on paper), and no stretch is drawn both visible and hidden.
#[test]
fn seeded_views_of_the_sample_stay_in_bounds_and_apart() {
    let mut s = Session::default();
    s.run_script(&crate::sample::script()).unwrap();
    let pick = |seed: u64, salt: u64, n: usize| ((seed ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) as usize % n;
    for seed in 1..=12u64 {
        let chord = [0.005, 0.01, 0.05, 0.2][pick(seed, 1, 4)];
        let view = ["front", "back", "top", "bottom", "left", "right", "iso"][pick(seed, 2, 7)];
        let scale = ["1:5", "1:2", "1:1", "2:1"][pick(seed, 3, 4)];
        let style = ["visible", "visible_hidden"][pick(seed, 4, 2)];
        let tangent = ["hide", "show"][pick(seed, 5, 2)];
        let p = json!({
            "view": view,
            "scale": scale,
            "chord_mm": chord,
            "visual_style": style,
            "scene": true,
            "tangent_edges": tangent,
        });
        let v = snap(&mut s, p.clone());
        let slack = chord + v["extents"]["error_mm"].as_f64().unwrap() * v["scale"]["factor"].as_f64().unwrap() + 1e-6;
        let (min, max) = (point(&v["boundary_mm"]["min"]), point(&v["boundary_mm"]["max"]));
        let inside = |q: &Vec2| q.x >= min.x - slack && q.x <= max.x + slack && q.y >= min.y - slack && q.y <= max.y + slack;
        let (visible, hidden) = (drawn(&v, "visible"), drawn(&v, "hidden"));
        assert!(!visible.is_empty(), "{p}");
        assert!(visible.iter().chain(&hidden).all(|seg| reach(seg).iter().all(inside)), "{p}: a line leaves the boundary");
        assert!(!hidden.iter().any(|h| visible.iter().any(|w| share_a_stretch(h, w))), "{p}: a stretch is both visible and hidden");
        let scene: solidcraft_geom::paper::Scene = serde_json::from_value(v["scene"].clone()).unwrap();
        assert!(scene.validate().is_ok(), "{p}");
        let pieces = v["lines"].as_array().unwrap().iter().flat_map(|l| l["pieces"].as_array().unwrap()).count();
        assert_eq!(strokes(&v).len(), pieces, "{p}: a stroke per piece");
        let widths: Vec<f64> = strokes(&v).iter().map(|(_, _, width)| *width).collect();
        assert!(widths.is_sorted(), "{p}: hidden lines first, under the visible ones");
    }
}

/// `chord_mm` and `max_error_mm` out of range are refused by name; a `max_error_mm` the lines can't
/// certify is refused with the count of pieces short of it: a cylinder's rims in iso are bounded by
/// the chord, 0.01, so they don't certify 0.001.
#[test]
fn chords_and_accuracy_checks_out_of_range_are_refused() {
    let mut s = block();
    let doc = s.doc.clone();
    for (p, says) in [
        (json!({"chord_mm": 0}), "chord_mm is a number from"),
        (json!({"chord_mm": 0.00099}), "chord_mm is a number from"),
        (json!({"chord_mm": 1.0001}), "chord_mm is a number from"),
        (json!({"chord_mm": "fine"}), "chord_mm"),
        (json!({"max_error_mm": 0}), "max_error_mm is a number above 0"),
        (json!({"max_error_mm": -1}), "max_error_mm is a number above 0"),
        (json!({"max_error_mm": null}), "max_error_mm"),
    ] {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(says), "{p}: {e}");
        assert!(std::sync::Arc::ptr_eq(&doc, &s.doc), "{p}");
    }
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"diameter": 20, "height": 30}));
    let e = refused(&mut s, json!({"view": "iso", "scale": "1:1", "max_error_mm": 0.001}));
    assert!(e.contains("accuracy not met: ") && e.contains(" bounded and 0 sampled pieces") && !e.contains("from the mesh"), "{e}");
    snap(&mut s, json!({"view": "iso", "scale": "1:1", "max_error_mm": 0.01}));
}

/// A stadium of cubic segments, two straight sides and four quarter circles, extruded 20: seen from
/// the front, its straight sides meet its round ends at tangent edges, drawn, and saying they are
/// tangent, only when the view shows tangent edges. (The kernel marks a join smooth only between
/// faces swept from cubic segments, and one between two pieces of one surface, as in the middle of
/// a round end, is a seam, never drawn; a fillet's round keeps its edges, as the viewport draws it.)
#[test]
fn tangent_edges_are_drawn_only_when_the_view_shows_them() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Stadium"}));
    // Quarter circles of radius 10 as cubics, their handles 10 x 4(sqrt 2 - 1)/3 = 5.5228 long.
    for points in [
        [[0.0, -10.0], [10.0, -10.0], [20.0, -10.0], [30.0, -10.0]],
        [[30.0, -10.0], [35.5228, -10.0], [40.0, -5.5228], [40.0, 0.0]],
        [[40.0, 0.0], [40.0, 5.5228], [35.5228, 10.0], [30.0, 10.0]],
        [[30.0, 10.0], [20.0, 10.0], [10.0, 10.0], [0.0, 10.0]],
        [[0.0, 10.0], [-5.5228, 10.0], [-10.0, 5.5228], [-10.0, 0.0]],
        [[-10.0, 0.0], [-10.0, -5.5228], [-5.5228, -10.0], [0.0, -10.0]],
    ] {
        run(&mut s, "sketch.spline.control_point", json!({"points": points}));
    }
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Stadium", "distance": 20, "body_name": "Stadium"}));
    let tangent = |v: &Value| v["lines"].as_array().unwrap().iter().filter(|l| l["source"]["tangent"] == true).count();
    let v = snap(&mut s, json!({"scale": "1:1", "tangent_edges": "show"}));
    assert!(tangent(&v) > 0, "{v}");
    let v = snap(&mut s, json!({"scale": "1:1"}));
    assert_eq!(tangent(&v), 0, "{v}");
    // The round ends' middles are seams, never drawn: the outline there is the round's silhouette,
    // 20 mm tall at x -10 and 40 (185 and 235 on the sheet).
    for x in [185.0, 235.0] {
        let upright = segments(&v, "visible").into_iter().filter(|(a, b)| (a.x - x).abs() < 1e-6 && (b.x - x).abs() < 1e-6);
        let tall: f64 = upright.map(|(a, b)| a.dist(b)).sum();
        assert!((tall - 20.0).abs() < 1e-6, "{x}: {tall}: {v}");
    }
}

/// Seen from above, the hole's rim is drawn as its exact circle: radius 5 round the sheet's
/// centre, analytic, all the way round. Its two rims lie one over the other, so one is covered;
/// the plate's outline is still its four straight sides.
#[test]
fn a_hole_seen_end_on_is_its_exact_circle() {
    let mut s = plate();
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1"}));
    let schema = output_schema_for::<SnapshotResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &v), "{v}");
    let rims = arcs(&v, "visible");
    assert!(!rims.is_empty(), "{v}");
    for (centre, radius, _, accuracy) in &rims {
        assert!(centre.dist(Vec2::new(210.0, 148.5)) < 1e-9 && (radius - 5.0).abs() < 1e-9 && accuracy == "analytic", "{v}");
    }
    let round: f64 = rims.iter().map(|(_, _, sweep, _)| sweep.abs()).sum();
    assert!((round - std::f64::consts::TAU).abs() < 1e-9, "{round}: {v}");
    assert!(v["status"]["covered_pieces"].as_u64().unwrap() > 0, "{v}");
    assert!((drawn_length(&v, "visible") - 160.0).abs() < 1e-6, "{v}");
}

/// A bar across the plate, above the hole, hides the rim where it passes over: the rim is cut where
/// the bar's sides cross it (2 mm either side of its centre), 4 asin 0.4 of it hidden and the rest
/// visible. The rim is cut where its exact curve crosses the bar's sides, so the arcs are exact.
#[test]
fn a_rim_cut_where_its_visibility_changes_is_exact() {
    let mut s = plate();
    run(&mut s, "solid.box", json!({"length": 30, "width": 4, "height": 2, "corner": [5, 18, 10], "body_name": "Bar", "operation": "new"}));
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1", "visual_style": "visible_hidden"}));
    let hidden = 4.0 * 0.4f64.asin();
    for (visibility, want) in [("hidden", hidden), ("visible", std::f64::consts::TAU - hidden)] {
        let rims = arcs(&v, visibility);
        let on_rim = |(c, r, _, a): &(Vec2, f64, f64, Value)| c.dist(Vec2::new(210.0, 148.5)) < 1e-9 && (r - 5.0).abs() < 1e-9 && a == "analytic";
        assert!(!rims.is_empty() && rims.iter().all(on_rim), "{visibility}: {v}");
        let round: f64 = rims.iter().map(|(_, _, sweep, _)| sweep.abs()).sum();
        assert!((round - want).abs() < 1e-9, "{visibility}: {round}, not {want}: {v}");
    }
}

/// A part with a hole placed three times, the third turned a quarter turn: all three are drawn
/// exact, nothing meshed, and each hole is an exact circle where its own placement puts it, 50 mm
/// apart on the sheet round its centre.
#[test]
fn each_placements_hole_is_its_own_exact_circle() {
    let mut s = Session::default();
    let first = run(&mut s, "component.create", json!({"name": "Part"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "body_name": "Plate", "operation": "new"}));
    run(&mut s, "solid.cylinder", json!({"diameter": 10, "height": 7, "base": [20, 20, -1], "operation": "cut", "targets": ["Plate"]}));
    run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [50, 0, 0]}));
    let turned = run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [140, 0, 0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": turned, "space": "world", "axis": [0, 0, 1], "angle": "90 deg", "origin": [140, 0, 0]}));
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1"}));
    assert_eq!((&v["status"]["provenance"], &v["stats"]["shared_meshes"]), (&json!("hidden_lines"), &json!(0)), "{v}");
    let mut round: std::collections::BTreeMap<i64, f64> = std::collections::BTreeMap::new();
    for (centre, radius, sweep, accuracy) in arcs(&v, "visible") {
        assert!((centre.y - 148.5).abs() < 1e-9 && (radius - 5.0).abs() < 1e-9 && accuracy == "analytic", "{v}");
        *round.entry(centre.x.round() as i64).or_default() += sweep.abs();
    }
    assert_eq!(round.keys().copied().collect::<Vec<_>>(), [160, 210, 260], "{v}");
    assert!(round.values().all(|r| (r - std::f64::consts::TAU).abs() < 1e-9), "{v}");
}

/// Every stroke of `v`'s paper scene: its path, whether it is closed, and its width.
fn strokes(v: &Value) -> Vec<(Vec<solidcore_record::Seg2>, bool, f64)> {
    use solidcraft_geom::paper::{Item, Scene};
    let scene: Scene = serde_json::from_value(v["scene"].clone()).unwrap();
    let items = scene.groups.into_iter().flat_map(|g| g.items);
    items
        .filter_map(|item| match item {
            Item::Stroke { path, closed, width_mm, .. } => Some((path, closed, width_mm)),
            Item::Fill { .. } | Item::Text(_) | Item::Picture(_) => None,
        })
        .collect()
}

/// The block's front view at 1:2 as a paper scene of the A3 sheet: one group of the view's kind,
/// object 0, its strokes open and 0.5 mm wide along the 120 mm outline, and the SVG writer takes
/// it as one group with a path per stroke. Without `scene` the reply has none.
#[test]
fn a_views_scene_is_its_visible_lines_as_strokes() {
    let mut s = block();
    assert!(snap(&mut s, json!({"scale": "1:2"})).get("scene").is_none());
    let v = snap(&mut s, json!({"scale": "1:2", "scene": true}));
    let schema = output_schema_for::<SnapshotResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &v), "{v}");
    let groups = &v["scene"]["groups"];
    let head = (&v["scene"]["size_mm"], groups.as_array().map(Vec::len), &groups[0]["object"], &groups[0]["kind"]);
    assert_eq!(head, (&json!([420.0, 297.0]), Some(1), &json!(0), &json!("view")), "{v}");
    let strokes = strokes(&v);
    assert!(strokes.iter().all(|(_, closed, width)| !closed && *width == 0.5), "{v}");
    let length: f64 = strokes.iter().flat_map(|(path, _, _)| path).map(|seg| seg.start().dist(seg.end())).sum();
    assert!((length - 120.0).abs() < 1e-6, "{v}");
    let scene: solidcraft_geom::paper::Scene = serde_json::from_value(v["scene"].clone()).unwrap();
    let (svg, _) = solidcraft_io::paper::paper_svg(&scene.prepare().unwrap(), &NoPictures, DEFAULT_DPI).unwrap();
    let svg = String::from_utf8(svg).unwrap();
    let counts = (svg.matches("<g data-object=\"0\" data-kind=\"view\">").count(), svg.matches("<path").count());
    assert_eq!(counts, (1, strokes.len()), "{svg}");
}

/// The pocket behind the front face as a paper scene at 1:1: its four hidden edges are dashed 0.25 mm
/// strokes under the block's outline, 100 mm of them beside its 240 mm, and the SVG writer dashes
/// those and only those. In `visible` the same view has no dashes.
#[test]
fn a_views_hidden_lines_are_dashed_strokes_under_its_outline() {
    use solidcraft_geom::paper::{Dashes, Item, Scene};
    let mut s = block();
    run(&mut s, "solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [30, 40, 5], "operation": "cut", "targets": ["Block"]}));
    let v = snap(&mut s, json!({"scale": "1:1", "scene": true, "visual_style": "visible_hidden"}));
    let schema = output_schema_for::<SnapshotResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &v), "{v}");
    let scene: Scene = serde_json::from_value(v["scene"].clone()).unwrap();
    let mut dashed = (0usize, 0.0);
    let mut solid = 0.0;
    for item in &scene.groups[0].items {
        let Item::Stroke { path, width_mm, pattern, .. } = item else { panic!("{item:?}") };
        let length: f64 = path.iter().map(|seg| seg.length()).sum();
        match pattern {
            Some(dashes) => {
                assert_eq!((dashes, *width_mm), (&Dashes { lengths_mm: vec![3.0, 0.75], phase_mm: 0.0 }, 0.25), "{v}");
                assert_eq!(solid, 0.0, "a dashed stroke after a solid one would paint over it: {v}");
                dashed = (dashed.0 + 1, dashed.1 + length);
            }
            None => {
                assert_eq!(*width_mm, 0.5, "{v}");
                solid += length;
            }
        }
    }
    assert_eq!(dashed.0, 4, "{v}");
    assert!((dashed.1 - 100.0).abs() < 1e-6 && (solid - 240.0).abs() < 1e-6, "{dashed:?} {solid}");
    let (svg, _) = solidcraft_io::paper::paper_svg(&scene.prepare().unwrap(), &NoPictures, DEFAULT_DPI).unwrap();
    let svg = String::from_utf8(svg).unwrap();
    // Under the round caps each dash is stroked a line width shorter about its middle: 2.75 and 1,
    // from 3.75 - 0.125 into the pattern.
    assert_eq!(svg.matches("stroke-dasharray=\"2.75 1\" stroke-dashoffset=\"3.625\"").count(), 4, "{svg}");
    let plain = snap(&mut s, json!({"scale": "1:1", "scene": true}));
    assert!(strokes(&plain).iter().all(|(_, _, width)| *width == 0.5), "{plain}");
    assert_eq!(plain["scene"].to_string().matches("lengths_mm").count(), 0, "{plain}");
}

/// The plate's hole in the scene, from above at 1:1: its rim's strokes are arcs of radius 5 round
/// the sheet's centre, 2π × 5 mm in all, and a stroke that is a whole circle is closed.
#[test]
fn a_holes_rim_is_arcs_in_the_scene() {
    let mut s = plate();
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1", "scene": true}));
    let mut round = 0.0;
    for (path, closed, _) in strokes(&v) {
        for seg in &path {
            if let solidcore_record::Seg2::Arc { center, radius, sweep, .. } = *seg {
                assert!(center.dist(Vec2::new(210.0, 148.5)) < 1e-9 && (radius - 5.0).abs() < 1e-9, "{v}");
                round += radius * sweep.abs();
                let whole = path.len() == 1 && (sweep.abs() - std::f64::consts::TAU).abs() < 1e-9;
                assert!(closed || !whole, "a whole circle is closed: {v}");
            }
        }
    }
    assert!((round - std::f64::consts::TAU * 5.0).abs() < 1e-6, "{round}: {v}");
}

/// `scene` is true or false.
#[test]
fn a_bad_scene_flag_is_refused() {
    let mut s = block();
    let doc = s.doc.clone();
    for (p, says) in [(json!({"scene": "yes"}), "scene"), (json!({"scene": 1}), "scene"), (json!({"scene": null}), "scene")] {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(says), "{p}: {e}");
        assert!(std::sync::Arc::ptr_eq(&doc, &s.doc), "{p}");
    }
}

/// A 40 x 40 x 5 mm part from the origin, and a 20 x 20 x 10 mm part over its right side at
/// x 30..50, y 10..30, from height `z`, set as reference. The session and the two occurrences.
fn plate_and_block(z: f64) -> (Session, [u64; 2]) {
    let mut s = Session::default();
    let part = |s: &mut Session, name: &str, size: [f64; 3], body: &str| {
        let occurrence = run(s, "component.create", json!({"name": name}))["occurrence"].as_u64().unwrap();
        run(s, "solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "body_name": body, "operation": "new"}));
        run(s, "component.activate", json!({"component": 0}));
        occurrence
    };
    let plate = part(&mut s, "PlatePart", [40.0, 40.0, 5.0], "Plate");
    let block = part(&mut s, "BlockPart", [20.0, 20.0, 10.0], "Block");
    run(&mut s, "occurrence.move", json!({"occurrence": block, "translate": [30, 10, z]}));
    run(&mut s, "occurrence.bom_type", json!({"occurrence": block, "bom_type": "reference"}));
    (s, [plate, block])
}

/// How long `v`'s model lines, or its reference lines, at `visibility` are altogether, on the sheet.
fn role_length(v: &Value, reference: bool, visibility: &str) -> f64 {
    let lines: Vec<Value> = v["lines"].as_array().unwrap().iter().filter(|l| l["reference"] == reference).cloned().collect();
    drawn_length(&json!({"lines": lines}), visibility)
}

fn assert_lengths(v: &Value, reference: bool, visible: f64, hidden: f64) {
    let got = (role_length(v, reference, "visible"), role_length(v, reference, "hidden"));
    assert!((got.0 - visible).abs() < 1e-6 && (got.1 - hidden).abs() < 1e-6, "{reference}: {got:?}, not ({visible}, {hidden})");
}

/// Worked by hand, top view at 1:1 showing all of the block. The plate's outline is
/// 160 mm, 20 of it (x = 40, y 10..30) under the block when the block is above; the block's is 80,
/// and when it is below, its parts at x 30..40 are under the plate: 40 hidden and 40 visible.
/// Reference parts cover the model only when told, and the plate always covers the block.
#[test]
fn reference_lines_are_their_own_and_cover_the_model_only_when_told() {
    let view = |hides: bool| {
        json!({"view": "top", "scale": "1:1", "visual_style": "visible_hidden", "reference_style": "visible_hidden",
               "reference_hides_model": hides, "reference_shown_percent": 100})
    };
    let (mut above, _) = plate_and_block(5.0);
    let v = snap(&mut above, view(false));
    assert_lengths(&v, false, 160.0, 0.0);
    assert_lengths(&v, true, 80.0, 0.0);
    let v = snap(&mut above, view(true));
    assert_lengths(&v, false, 140.0, 20.0);
    assert_lengths(&v, true, 80.0, 0.0);
    let (mut below, _) = plate_and_block(-10.0);
    for hides in [false, true] {
        let v = snap(&mut below, view(hides));
        assert_lengths(&v, false, 160.0, 0.0);
        assert_lengths(&v, true, 40.0, 40.0);
    }
}

/// Each style asks for its own hidden lines: reference ones only with `reference_style`, the
/// model's only with `visual_style`.
#[test]
fn each_part_of_a_view_draws_hidden_lines_by_its_own_style() {
    let (mut above, _) = plate_and_block(5.0);
    let hides = json!({"view": "top", "scale": "1:1", "reference_hides_model": true, "reference_shown_percent": 100});
    let v = snap(&mut above, hides.clone());
    assert_lengths(&v, false, 140.0, 0.0);
    let (mut below, _) = plate_and_block(-10.0);
    let v = snap(&mut below, hides);
    assert_lengths(&v, true, 40.0, 0.0);
    let v = snap(&mut below, json!({"view": "top", "scale": "1:1", "reference_style": "visible_hidden", "reference_shown_percent": 100}));
    assert_lengths(&v, true, 40.0, 40.0);
}

/// With model parts there, the boundary starts at theirs and reference lines stop at it. The block
/// passes only the plate's right side, so that side alone grows, to x 50 at 100%: the block keeps
/// x 30..40 at 0% (20 + 10 + 10), x 30..45 at 50% (20 + 15 + 15) and all of itself at 100%. The model
/// stays where it is throughout, so a view stays lined up with its projected views.
#[test]
fn reference_parts_grow_the_boundary_on_their_own_side_as_far_as_asked() {
    let (mut s, _) = plate_and_block(5.0);
    let at = |s: &mut Session, percent: f64| snap(s, json!({"view": "top", "scale": "1:1", "reference_shown_percent": percent}));
    let model_lines = |v: &Value| v["lines"].as_array().unwrap().iter().filter(|l| l["reference"] == false).cloned().collect::<Vec<_>>();
    let none = at(&mut s, 0.0);
    assert_box(&none, [190.0, 128.5], [230.0, 168.5]);
    assert_eq!(none["source"]["bodies"], 2, "{none}");
    assert_lengths(&none, true, 40.0, 0.0);
    let half = at(&mut s, 50.0);
    assert_box(&half, [190.0, 128.5], [235.0, 168.5]);
    assert_lengths(&half, true, 50.0, 0.0);
    let all = at(&mut s, 100.0);
    assert_box(&all, [190.0, 128.5], [240.0, 168.5]);
    assert_lengths(&all, true, 80.0, 0.0);
    assert_eq!(model_lines(&none), model_lines(&half));
    assert_eq!(model_lines(&none), model_lines(&all));
    // A side no reference part passes is the model's, exactly, whatever the percent, and the side it
    // passes only ever grows.
    let mut reach = f64::MIN;
    for percent in [0.0, 1e-9, 0.1, 33.3, 50.0, 99.999, 100.0] {
        let v = at(&mut s, percent);
        assert_eq!(none["boundary_mm"]["min"], v["boundary_mm"]["min"], "{percent}: {v}");
        let right = v["boundary_mm"]["max"][0].as_f64().unwrap();
        assert!(right >= reach && right <= 240.0, "{percent}: {v}");
        reach = right;
    }
}

/// Left out and 0 are one picture, reply for reply, apart from what working it out took.
#[test]
fn no_reference_shown_is_the_same_picture_as_not_asking() {
    let (mut s, _) = plate_and_block(5.0);
    let reply = |s: &mut Session, p: Value| {
        let mut v = snap(s, p);
        v.as_object_mut().unwrap().remove("stats");
        v
    };
    let plain = reply(&mut s, json!({"view": "top", "scale": "1:1", "scene": true}));
    assert_eq!(plain, reply(&mut s, json!({"view": "top", "scale": "1:1", "scene": true, "reference_shown_percent": 0})));
}

/// A part many times the model's size: at 100% the box holds it all and a fitted scale takes it in,
/// and a scale that can't holds it is refused by name, while 0% still draws. A view with only
/// reference parts is measured by them already, so the percent changes nothing.
#[test]
fn a_reference_part_much_bigger_than_the_model_is_held_or_refused() {
    let mut s = Session::default();
    let add = |s: &mut Session, name: &str, size: f64, body: &str| {
        let occurrence = run(s, "component.create", json!({"name": name}))["occurrence"].as_u64().unwrap();
        run(s, "solid.box", json!({"length": size, "width": size, "height": 5, "body_name": body, "operation": "new"}));
        run(s, "component.activate", json!({"component": 0}));
        occurrence
    };
    add(&mut s, "PlatePart", 10.0, "Plate");
    let big = add(&mut s, "BigPart", 20000.0, "Big");
    run(&mut s, "occurrence.bom_type", json!({"occurrence": big, "bom_type": "reference"}));
    let fitted = |s: &mut Session, percent: f64| snap(s, json!({"view": "top", "reference_shown_percent": percent}));
    let (near, far) = (fitted(&mut s, 0.0), fitted(&mut s, 100.0));
    assert_ne!(near["scale"], far["scale"], "{far}");
    // At 0% the view shows the two sides of the big part that cross the model, cut off at the view's
    // edges; at 100% all four.
    let reference_lines = |v: &Value| v["lines"].as_array().unwrap().iter().filter(|l| l["reference"] == true).count();
    assert_eq!((reference_lines(&near), reference_lines(&far)), (2, 4), "{far}");
    // Fitted round the model's middle, the side it grows on still stays on the sheet.
    let position = (point(&near["boundary_mm"]["min"]) + point(&near["boundary_mm"]["max"])) * 0.5;
    let (low, high) = (point(&far["boundary_mm"]["min"]), point(&far["boundary_mm"]["max"]));
    assert!(low.x >= 0.0 && low.y >= 0.0 && high.x <= position.x * 2.0 && high.y <= position.y * 2.0, "{far}");
    let refused =
        s.execute("drawing.view.snapshot", &json!({"view": "top", "scale": "100:1", "reference_shown_percent": 100})).unwrap_err().to_string();
    assert!(refused.contains("reaches past"), "{refused}");
    snap(&mut s, json!({"view": "top", "scale": "100:1"}));

    let (mut only, [plate, _]) = plate_and_block(5.0);
    run(&mut only, "occurrence.bom_type", json!({"occurrence": plate, "bom_type": "reference"}));
    let at = |s: &mut Session, percent: f64| snap(s, json!({"view": "top", "scale": "1:1", "reference_shown_percent": percent}));
    let (none, all) = (at(&mut only, 0.0), at(&mut only, 100.0));
    assert_eq!((&none["boundary_mm"], &none["lines"]), (&all["boundary_mm"], &all["lines"]));
}

/// Reference lines test against everything: with both parts reference, the block hides 20 mm of the
/// plate, the setting off.
#[test]
fn reference_parts_hide_each_other_whatever_the_setting() {
    let (mut s, [plate, _]) = plate_and_block(5.0);
    run(&mut s, "occurrence.bom_type", json!({"occurrence": plate, "bom_type": "reference"}));
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1", "reference_style": "visible_hidden"}));
    assert_lengths(&v, true, 220.0, 20.0);
}

/// A reference part's edge on a model part's: the block's right edge moved onto the plate's. Merged
/// apart, each keeps its whole outline, so the model's line is drawn in the model's width. So does a
/// block as tall as the plate and standing in it, over its right edge (parts may overlap): the
/// block's right edge is the plate's, one line seen by both, and the block's stays whole although the
/// plate is tested against it.
#[test]
fn a_reference_line_never_takes_a_stretch_from_a_model_line() {
    let (mut s, [_, block]) = plate_and_block(5.0);
    run(&mut s, "occurrence.move", json!({"occurrence": block, "translate": [-10, 0, 0]}));
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1"}));
    assert_lengths(&v, false, 160.0, 0.0);
    assert_lengths(&v, true, 80.0, 0.0);
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "PlatePart"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "body_name": "Plate", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let inside = run(&mut s, "component.create", json!({"name": "BlockPart"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 5, "corner": [20, 10, 0], "body_name": "Block", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.bom_type", json!({"occurrence": inside, "bom_type": "reference"}));
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1"}));
    assert_lengths(&v, false, 160.0, 0.0);
    assert_lengths(&v, true, 80.0, 0.0);
}

/// A plate, a hinge holding a pin, and the pin placed again at the top: with the hinge a reference
/// component, the pin is a reference part through the hinge and a model part on its own.
#[test]
fn an_assemblys_type_reaches_its_parts_through_that_path_only() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 3}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.create", json!({"name": "Hinge", "activate": false}));
    run(&mut s, "component.create", json!({"name": "Pin", "parent": "Hinge"}));
    run(&mut s, "solid.cylinder", json!({"diameter": 4, "height": 30}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.copy", json!({"component": "Pin", "translate": [0, 50, 0]}));
    run(&mut s, "component.metadata.set", json!({"component": "Hinge", "bom_type": "reference"}));
    let through = |line: &Value| line["occurrence"].as_array().unwrap().len() == 2;
    let all = snap(&mut s, json!({}));
    let lines = all["lines"].as_array().unwrap();
    assert!(lines.iter().any(through) && !lines.iter().all(through), "{all}");
    assert!(lines.iter().all(|l| l["reference"] == through(l)), "{all}");
}

/// A view of only reference parts is measured by them and draws them whole.
#[test]
fn a_view_of_only_reference_parts_draws_them() {
    let (mut s, [plate, _]) = plate_and_block(5.0);
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1", "hide": [[plate]]}));
    assert_box(&v, [200.0, 138.5], [220.0, 158.5]);
    assert_lengths(&v, true, 80.0, 0.0);
    assert!(v["lines"].as_array().unwrap().iter().all(|l| l["reference"] == true), "{v}");
}

/// An assembly's type reaches every part in it, and so does a component's; a part's own type says
/// nothing about the assembly round it, or about what else is placed.
#[test]
fn a_reference_assembly_makes_its_parts_reference() {
    let mut s = Session::default();
    let outer = run(&mut s, "component.create", json!({"name": "Outer"}))["occurrence"].as_u64().unwrap();
    let inner = run(&mut s, "component.create", json!({"name": "Inner"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let reference = |s: &Session| crate::drawing_view::shown(s).unwrap().bodies.iter().map(|b| (b.path.clone(), b.reference)).collect::<Vec<_>>();
    assert_eq!(reference(&s), [(vec![outer, inner], false)]);
    run(&mut s, "occurrence.bom_type", json!({"occurrence": outer, "bom_type": "reference"}));
    assert_eq!(reference(&s), [(vec![outer, inner], true)]);
    run(&mut s, "occurrence.bom_type", json!({"occurrence": outer, "bom_type": null}));
    assert_eq!(reference(&s), [(vec![outer, inner], false)]);
    run(&mut s, "component.metadata.set", json!({"component": "Outer", "bom_type": "reference"}));
    assert_eq!(reference(&s), [(vec![outer, inner], true)]);
    // Its own type wins over its component's.
    run(&mut s, "occurrence.bom_type", json!({"occurrence": outer, "bom_type": "normal"}));
    assert_eq!(reference(&s), [(vec![outer, inner], false)]);
    run(&mut s, "occurrence.bom_type", json!({"occurrence": inner, "bom_type": "reference"}));
    assert_eq!(reference(&s), [(vec![outer, inner], true)]);
}

/// A design's own bodies are never reference, and a view with no reference parts draws the same
/// lines whichever way the setting is.
#[test]
fn no_reference_parts_means_the_setting_changes_nothing() {
    let (mut s, _) = two_cubes();
    let plain = snap(&mut s, json!({"scale": "1:1"}));
    let set = snap(&mut s, json!({"scale": "1:1", "reference_hides_model": true}));
    assert_eq!(plain["lines"], set["lines"]);
    assert!(plain["lines"].as_array().unwrap().iter().all(|l| l["reference"] == false), "{plain}");
}

/// The snapshot's scene draws reference parts as saved views do: thin, and first. The reference
/// block is drawn exact, as the plate is, so the view certifies any accuracy.
#[test]
fn a_scene_draws_reference_parts_thin_under_the_model() {
    let (mut s, _) = plate_and_block(5.0);
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1", "scene": true, "reference_shown_percent": 100}));
    let widths: Vec<f64> = strokes(&v).iter().map(|(_, _, width)| *width).collect();
    let thin = widths.iter().take_while(|w| **w == LineKind::Thin.width_mm(LineGroup::DEFAULT)).count();
    assert!(thin > 0 && widths[thin..].iter().all(|w| *w == LineKind::Visible.width_mm(LineGroup::DEFAULT)), "{widths:?}");
    assert_eq!(v["status"]["provenance"], "hidden_lines", "{v}");
    snap(&mut s, json!({"view": "top", "scale": "1:1", "reference_shown_percent": 100, "max_error_mm": 0.0001}));
}

/// A reference part's arcs stay exact inside the view and are cut on their chords at its edge. On
/// the plate stand a 10 mm pin round (20, 20) and a 10 mm peg round (40, 20), both reference parts,
/// seen from above at 1:1 with no room given to reference parts. The pin's rim is one whole analytic
/// circle round the sheet's centre (210, 148.5). The peg keeps the half of its rim inside the plate's
/// x 40 (230 on the sheet): one arc on its circle, its ends where its chords (within 0.01 mm) cross
/// x 40, so sampled, as the mesh projection draws it.
#[test]
fn a_reference_parts_arcs_stay_exact_inside_the_view_and_are_cut_at_its_edge() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "PlatePart"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "body_name": "Plate", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    for (name, x) in [("Pin", 20), ("Peg", 40)] {
        let occurrence = run(&mut s, "component.create", json!({"name": format!("{name}Part")}))["occurrence"].as_u64().unwrap();
        run(&mut s, "solid.cylinder", json!({"diameter": 10, "height": 10, "base": [x, 20, 5], "body_name": name, "operation": "new"}));
        run(&mut s, "component.activate", json!({"component": 0}));
        run(&mut s, "occurrence.bom_type", json!({"occurrence": occurrence, "bom_type": "reference"}));
    }
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1"}));
    assert_eq!(v["status"]["provenance"], "hidden_lines", "{v}");
    let of = |body: &str| json!({"lines": v["lines"].as_array().unwrap().iter().filter(|l| l["body"] == body).cloned().collect::<Vec<_>>()});
    let pin = of("Pin");
    let rims = arcs(&pin, "visible");
    let [(centre, radius, sweep, kind)] = rims.as_slice() else { panic!("one rim: {pin}") };
    assert!(centre.dist(Vec2::new(210.0, 148.5)) < 1e-9 && (radius - 5.0).abs() < 1e-9, "{pin}");
    assert!((sweep.abs() - std::f64::consts::TAU).abs() < 1e-9 && kind == "analytic", "{pin}");
    assert!(segments(&pin, "visible").is_empty(), "{pin}");
    let peg = of("Peg");
    let rims = arcs(&peg, "visible");
    let [(centre, radius, sweep, kind)] = rims.as_slice() else { panic!("one arc: {peg}") };
    assert!(centre.dist(Vec2::new(230.0, 148.5)) < 1e-9 && (radius - 5.0).abs() < 1e-9, "{peg}");
    // A cut on a chord is at most its sag (0.01 mm) off the circle: that far round on either end.
    let off = 2.0 * (0.01f64 / 5.0).asin();
    assert!((sweep.abs() - std::f64::consts::PI).abs() <= off && kind == "sampled", "{sweep}: {peg}");
    assert!(segments(&peg, "visible").is_empty(), "{peg}");
}

/// A scene's hidden reference lines are dashed as the model's are, by the reference parts' own
/// style: with the block below the plate, its 40 mm under the plate (worked by hand above) is drawn
/// dashed, 0.25 mm wide, before every solid stroke; with the reference parts in `visible`, not at all.
#[test]
fn a_scenes_hidden_reference_lines_are_dashed_by_their_own_style() {
    use solidcraft_geom::paper::{Item, Scene};
    let (mut s, _) = plate_and_block(-10.0);
    let mut dashed = |style: &str| {
        let v = snap(&mut s, json!({"view": "top", "scale": "1:1", "scene": true, "reference_style": style, "reference_shown_percent": 100}));
        let schema = output_schema_for::<SnapshotResult>();
        assert!(conforms(schema.as_value(), schema.as_value(), &v), "{v}");
        let scene: Scene = serde_json::from_value(v["scene"].clone()).unwrap();
        let (mut length, mut solid) = (0.0, false);
        for item in &scene.groups[0].items {
            let Item::Stroke { path, width_mm, pattern, .. } = item else { panic!("{item:?}") };
            match pattern {
                Some(_) => {
                    assert!(!solid && *width_mm == 0.25, "{v}");
                    length += path.iter().map(|seg| seg.length()).sum::<f64>();
                }
                None => solid = true,
            }
        }
        length
    };
    assert!((dashed("visible_hidden") - 40.0).abs() < 1e-6);
    assert_eq!(dashed("visible"), 0.0);
}

/// The readings of `measure` on `v`, each (value, label).
fn readings(v: &Value) -> Vec<(f64, String)> {
    let value = |m: &Value| m.get("value_mm").or(m.get("value_rad")).and_then(Value::as_f64).unwrap();
    v["measurements"].as_array().unwrap().iter().map(|m| (value(m), m["label"].as_str().unwrap().to_string())).collect()
}

/// From (0, 0, 0) to (30, 40, 0) seen from above, 1:2: 30 along x, 40 along y, 50 as it runs, half
/// that on paper; a 45 degree line reads 45 degrees from the view's horizontal and from a line along
/// x; a reply with none says nothing about them. Nothing changes or is evaluated.
#[test]
fn a_views_measurements_are_read_as_it_shows_them() {
    let mut s = block();
    let (doc, evaluations) = (s.doc.clone(), s.evaluations);
    let (a, b) = (json!([0, 0, 0]), json!([30, 40, 0]));
    let v = snap(
        &mut s,
        json!({"view": "top", "scale": "1:2", "measure": [
            {"linear": {"along": "x", "from": a, "to": b}},
            {"linear": {"along": "y", "from": a, "to": b}},
            {"aligned": {"from": a, "to": b}},
            {"angle": {"edge": [[0, 0, 0], [1, 1, 0]], "from": "horizontal"}},
            {"angle": {"edge": [[0, 0, 0], [1, 1, 0]], "from": [[0, 0, 0], [1, 0, 0]]}},
        ]}),
    );
    let schema = crate::schema::output_schema_for::<crate::schema::drawing_view::SnapshotResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &v), "{v}");
    let got = readings(&v);
    let want = [(30.0, "30.00"), (40.0, "40.00"), (50.0, "50.00"), (std::f64::consts::FRAC_PI_4, "45.00°"), (std::f64::consts::FRAC_PI_4, "45.00°")];
    assert_eq!(got.len(), want.len());
    for ((value, label), (expected, text)) in got.iter().zip(want) {
        assert!((value - expected).abs() < 1e-9 && label == text, "{got:?}");
    }
    let first = &v["measurements"][0];
    assert_eq!(
        (&first["kind"], &first["paper_mm"], &first["meaning"], &first["foreshortened"]),
        (&json!("linear"), &json!(15.0), &json!("projected"), &json!(false)),
        "{v}"
    );
    assert_eq!(v["measurements"][3].get("value_mm"), None, "{v}");
    assert_eq!(snap(&mut s, json!({"view": "top"})).get("measurements"), None);
    assert!(Arc::ptr_eq(&doc, &s.doc) && s.evaluations == evaluations);
}

/// A 50 mm edge of a cube seen in iso reads 50 x sqrt(2/3) = 40.825 and says it is foreshortened;
/// the label takes the units and decimals asked for.
#[test]
fn an_edge_in_iso_reads_foreshortened() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 50, "width": 50, "height": 50}));
    let edge = json!({"aligned": {"from": [0, 0, 0], "to": [50, 0, 0]}});
    let v = snap(&mut s, json!({"view": "iso", "measure": [edge], "decimals": 3}));
    let got = readings(&v);
    assert!((got[0].0 - 50.0 * (2.0f64 / 3.0).sqrt()).abs() < 1e-9 && got[0].1 == "40.825", "{got:?}");
    assert_eq!(v["measurements"][0]["foreshortened"], true);
    let inch = json!({"aligned": {"from": [0, 0, 0], "to": [25.4, 0, 0]}});
    let v = snap(&mut s, json!({"view": "top", "measure": [inch], "units": "in", "decimals": 3}));
    assert_eq!(readings(&v)[0].1, "1.000", "{v}");
}

/// A 20 mm hole reads R10 where the view sees it round, on paper at the view's scale, and is refused
/// end on, by its place in the list.
#[test]
fn a_holes_radius_is_read_face_on_and_refused_end_on() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"diameter": 20, "height": 7, "base": [20, 20, -1], "operation": "cut", "targets": ["Plate"]}));
    let rim = json!({"radius": {"edge": [30, 20, 5]}});
    let v = snap(&mut s, json!({"view": "top", "scale": "2:1", "measure": [rim]}));
    let got = readings(&v);
    assert!(got.len() == 1 && (got[0].0 - 10.0).abs() < 1e-9 && got[0].1 == "R10.00", "{v}");
    let paper = v["measurements"][0]["paper_mm"].as_f64().unwrap();
    assert!((paper - 20.0).abs() < 1e-9 && v["measurements"][0]["status"] == "exact", "{v}");
    let e = refused(&mut s, json!({"view": "front", "measure": [{"aligned": {"from": [0, 0, 0], "to": [1, 0, 0]}}, rim]}));
    assert!(e.contains("measure[1]") && e.contains("isn't a circle in this view"), "{e}");
    let straight = json!({"radius": {"edge": [0, 20, 5]}});
    let e = refused(&mut s, json!({"view": "top", "measure": [straight]}));
    assert!(e.contains("measure[0]"), "{e}");
}

#[test]
fn hostile_measurements_are_refused() {
    let mut s = block();
    let doc = s.doc.clone();
    let one = json!({"aligned": {"from": [0, 0, 0], "to": [1, 0, 0]}});
    let many = vec![one.clone(); 101];
    let cases = [
        (json!({"measure": many}), "at most 100"),
        (json!({"measure": [{"aligned": {"from": [0, 0, 0], "to": [0, 0, 0]}}]}), "measure[0]"),
        (json!({"view": "top", "measure": [{"aligned": {"from": [0, 0, 0], "to": [0, 0, 5]}}]}), "measure[0]"),
        (json!({"measure": [{"aligned": {"from": [0, 0, 0], "to": [1e13, 0, 0]}}]}), "measure[0]"),
        (json!({"measure": [{"linear": {"along": "z", "from": [0, 0, 0], "to": [1, 0, 0]}}]}), "measure"),
        (json!({"measure": [{"aligned": {"from": [0, 0, 0], "to": ["a", 0, 0]}}]}), "measure"),
        (json!({"measure": [{"angle": {"edge": [[0, 0, 0], [1, 0, 0]], "from": "horizontal"}}]}), "measure[0]"),
        (json!({"measure": [{"radius": {"edge": [1e13, 0, 0]}}]}), "measure"),
        (json!({"measure": [{"nothing": {}}]}), "measure"),
        (json!({"measure": [one.clone()], "units": "parsec"}), "units"),
        (json!({"measure": [one.clone()], "decimals": 7}), "decimals"),
        (json!({"measure": [one], "decimals": -1}), "decimals"),
    ];
    for (p, says) in cases {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(says), "{p}: {e}");
        assert!(Arc::ptr_eq(&doc, &s.doc), "{p}");
    }
}

/// The occurrence paths `v`'s lines name, each once.
fn line_paths(v: &Value) -> Vec<Value> {
    let mut paths: Vec<Value> = v["lines"].as_array().unwrap().iter().map(|l| l["occurrence"].clone()).collect();
    paths.sort_by_key(|p| p.to_string());
    paths.dedup();
    paths
}

/// Two copies of a part: `source` with the first copy's path draws only that copy, 10 mm across
/// at 1:1; hiding the second copy draws the same; both together leave nothing and are refused.
#[test]
fn a_views_source_and_hide_choose_its_copies() {
    let (mut s, [first, second]) = two_cubes();
    for p in [json!({"scale": "1:1", "source": {"occurrences": [[first]]}}), json!({"scale": "1:1", "hide": [[second]]})] {
        let v = snap(&mut s, p.clone());
        assert_eq!(line_paths(&v), [json!([first])], "{p}: {v}");
        let (size, _) = boundary(&v);
        assert!((size.x - 10.0).abs() < 1e-6, "{p}: {v}");
    }
    let e = refused(&mut s, json!({"source": {"occurrences": [[first]]}, "hide": [[first]]}));
    assert!(e.contains("the source and hide leave no bodies to draw"), "{e}");
}

/// The empty path takes the design's own bodies only: the block made at the top of the design and
/// not the cube placed in a part.
#[test]
fn the_empty_path_is_the_designs_own_bodies() {
    let mut s = block();
    run(&mut s, "component.create", json!({"name": "Part"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [200, 0, 0], "body_name": "Cube", "operation": "new"}));
    let bodies = |v: &Value| {
        let mut names: Vec<Value> = v["lines"].as_array().unwrap().iter().map(|l| l["body"].clone()).collect();
        names.sort_by_key(|n| n.to_string());
        names.dedup();
        names
    };
    assert_eq!(bodies(&snap(&mut s, json!({"scale": "1:5"}))), [json!("Block"), json!("Cube")]);
    assert_eq!(bodies(&snap(&mut s, json!({"scale": "1:5", "source": {"occurrences": [[]]}}))), [json!("Block")]);
}

/// Paths the occurrences don't make, too many paths or ids, `[]` in `hide` and malformed values
/// are refused with their reason, and the design is left as it was.
#[test]
fn hostile_source_and_hide_paths_are_refused() {
    let (mut s, [first, _]) = two_cubes();
    let doc = s.doc.clone();
    let many: Vec<Vec<u64>> = vec![vec![first]; 1001];
    let deep: Vec<Vec<u64>> = vec![vec![first; 64]; 65];
    let cases = [
        (json!({"source": {"occurrences": [[999]]}}), "missing or mismatched parent"),
        (json!({"hide": [[first, first]]}), "repeats an id"),
        (json!({"source": {"occurrences": [vec![first; 65]]}}), "1..64 ids"),
        (json!({"hide": many}), "at most 1000 occurrence paths"),
        (json!({"source": {"occurrences": deep}}), "4096 ids in all"),
        (json!({"hide": [[]]}), "hide [] would leave out"),
        (json!({"hide": [[-1]]}), "hide"),
        (json!({"hide": [[1.5]]}), "hide"),
        (json!({"source": {"occurrences": [["a"]]}}), "source"),
        (json!({"source": null}), "source"),
        (json!({"source": {}}), "occurrences"),
        (json!({"source": {"occurrences": [], "extra": 1}}), "extra"),
    ];
    for (p, says) in cases {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(says), "{p}: {e}");
        assert!(std::sync::Arc::ptr_eq(&doc, &s.doc), "{p}");
    }
}

/// A view shows every body the design places, so a design whose assembly walk stops at its depth
/// limit before some occurrences is refused, and one that ends at the limit is drawn: a chain of
/// components one inside the next, one level at a time across the limit.
#[test]
fn a_view_of_an_assembly_past_the_walks_limit_is_refused() {
    let mut s = block();
    // The deepest occurrence's path is MAX_PLACEMENT_DEPTH + 1 long: walked, its children not.
    for level in 0..=MAX_PLACEMENT_DEPTH as u64 {
        nest(&mut s, level);
    }
    assert!(s.doc.placements_complete());
    snap(&mut s, json!({"scale": "1:1"}));
    nest(&mut s, MAX_PLACEMENT_DEPTH as u64 + 1);
    assert!(!s.doc.placements_complete());
    let e = refused(&mut s, json!({"scale": "1:1"}));
    assert!(e.contains("deeper than 64 levels or has more than 100000 placements"), "{e}");
}

/// A saved view's boundary, worked by hand for the block at 1:2 (front 50 x 10, right 25 x 10, the
/// right view's top 25 x 50): the front view round its point (150, 150), the right view at its 300
/// across and the front view's 150 up, the top view round its own point; the same boxes in first
/// angle, as each of the block's sides is the size of the one opposite. The extents are the
/// model's own millimetres, and inspect evaluates no model.
#[test]
fn a_saved_views_boundary_is_its_extents_at_its_scale_round_its_centre() {
    let (mut s, [front, right, top]) = three_views();
    let evaluations = s.evaluations;
    for projection in [Projection::Third, Projection::First] {
        set_projection(&mut s, projection);
        assert_box(&inspect_view(&mut s, front), [125.0, 145.0], [175.0, 155.0]);
        assert_box(&inspect_view(&mut s, right), [287.5, 145.0], [312.5, 155.0]);
        assert_box(&inspect_view(&mut s, top), [287.5, 225.0], [312.5, 275.0]);
    }
    let base = inspect_view(&mut s, front);
    let extents = &base["extents"];
    assert!((point(&extents["min"]) - Vec2::ZERO).len() < EPS && (point(&extents["max"]) - Vec2::new(100.0, 20.0)).len() < EPS, "{base}");
    assert_eq!((extents["status"].as_str(), extents["provenance"].as_str()), (Some("approximate"), Some("display_mesh")));
    assert_eq!(s.evaluations, evaluations, "no model is evaluated");
}

/// The right view follows the front view up and down when it moves, and keeps its own place
/// across; at its own scale (1:1, 50 x 20) it is still lined up through its centre, and the top
/// view projected from it takes that scale too. The largest scale still gives a finite box.
#[test]
fn a_lined_up_view_follows_its_parent() {
    let (mut s, [front, right, top]) = three_views();
    edit_view(&mut s, front, json!({"place": {"at": [160.0, 140.0]}}));
    assert_box(&inspect_view(&mut s, front), [135.0, 135.0], [185.0, 145.0]);
    assert_box(&inspect_view(&mut s, right), [287.5, 135.0], [312.5, 145.0]);
    edit_view(&mut s, right, json!({"scale": "1:1"}));
    assert_box(&inspect_view(&mut s, right), [275.0, 130.0], [325.0, 150.0]);
    assert_box(&inspect_view(&mut s, top), [275.0, 200.0], [325.0, 300.0]);
    edit_view(&mut s, top, json!({"scale": "1000:1"}));
    let (huge, centre) = boundary(&inspect_view(&mut s, top));
    assert!(huge.x.is_finite() && huge.x > 1e4 && (centre - Vec2::new(300.0, 250.0)).len() < 1e-6, "{huge:?} {centre:?}");
}

/// With nothing to measure a view gets a 20 x 20 mm placeholder round its centre, and says why:
/// its design places more than a view walks, or has no bodies.
#[test]
fn a_view_with_nothing_to_measure_gets_a_placeholder() {
    let placeholder = |s: &mut Session, view: u64, why: &str| {
        let v = inspect_view(s, view);
        assert_eq!((v["extents"]["status"].as_str(), v["extents"]["provenance"].as_str()), (Some("placeholder"), Some(why)), "{v}");
        v
    };
    let (mut s, [front, right, _]) = three_views();
    for level in 0..=MAX_PLACEMENT_DEPTH as u64 + 1 {
        nest(&mut s, level);
    }
    assert_box(&placeholder(&mut s, front, "past_walk_limit"), [140.0, 140.0], [160.0, 160.0]);
    assert_box(&placeholder(&mut s, right, "past_walk_limit"), [290.0, 140.0], [310.0, 160.0]);
    // One sketch: something to draw (an empty design isn't), and no bodies.
    let mut sketch_only = Session::default();
    run(&mut sketch_only, "sketch.create", json!({"plane": "XY"}));
    run(&mut sketch_only, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 5]}));
    run(&mut sketch_only, "sketch.finish", json!({}));
    let (mut sketch_only, [front, ..]) = three_views_of(sketch_only);
    assert_box(&placeholder(&mut sketch_only, front, "no_bodies"), [140.0, 140.0], [160.0, 160.0]);
}

/// The two copies of [`two_cubes`] in a drawing: a base view at 1:1 centred at (150, 150), and the
/// session, its sheet, the view and the two occurrences.
fn two_cubes_drawn() -> (Session, u64, [u64; 2]) {
    let (mut s, copies) = two_cubes();
    run(&mut s, "drawing.create", json!({}));
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let (_, view) = create(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "position_mm": [150, 150]}));
    (s, view, copies)
}

/// How wide view `id` is on its sheet, in paper millimetres.
fn width(s: &mut Session, id: u64) -> f64 {
    boundary(&inspect_view(s, id)).0.x
}

/// `source` and `hide` on a saved view: both copies are 40 mm across, one is 10 mm, and its
/// boundary stays centred on its place. Inspect says where each comes from, null follows again, and
/// an edit is one undo step that undoes.
#[test]
fn a_saved_views_source_and_hide_choose_what_it_measures() {
    let (mut s, view, [_, second]) = two_cubes_drawn();
    assert!((width(&mut s, view) - 40.0).abs() < 1e-6);
    let inspected = inspect_view(&mut s, view);
    assert_eq!(inspected["settings"]["occurrences"], json!({"value": null, "from": "default", "set": null}), "{inspected}");
    assert_eq!(inspected["settings"]["hide"], json!({"value": null, "from": "default", "set": null}), "{inspected}");

    let steps = s.undo.len();
    let reply = edit_view(&mut s, view, json!({"hide": [[second]]}));
    assert_eq!((reply["changed"].clone(), s.undo.len()), (json!(["hide"]), steps + 1));
    assert!((width(&mut s, view) - 10.0).abs() < 1e-6);
    assert_box(&inspect_view(&mut s, view), [145.0, 145.0], [155.0, 155.0]);
    let inspected = inspect_view(&mut s, view);
    assert_eq!(inspected["settings"]["hide"], json!({"value": [[second]], "from": "this", "set": [[second]]}), "{inspected}");

    let reply = edit_view(&mut s, view, json!({"hide": null, "source": {"occurrences": [[second]]}}));
    assert_eq!(reply["changed"], json!(["source", "hide"]));
    assert!((width(&mut s, view) - 10.0).abs() < 1e-6);
    edit_view(&mut s, view, json!({"source": null}));
    assert!((width(&mut s, view) - 40.0).abs() < 1e-6, "null follows the default again");

    run(&mut s, "edit.undo", json!({}));
    assert!((width(&mut s, view) - 10.0).abs() < 1e-6);
    assert_eq!(saved(&s, view).settings.occurrences.as_deref(), Some(&[vec![second]][..]));
}

/// A new base view takes its scale from what it shows: with the second copy hidden it fits one
/// 10 mm cube, so it comes out larger than the view of both.
#[test]
fn a_base_views_fitted_scale_is_of_what_it_shows() {
    let (mut s, _, [_, second]) = two_cubes_drawn();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let (both, _) = create(&mut s, BASE, json!({"sheet": sheet}));
    let (one, id) = create(&mut s, BASE, json!({"sheet": sheet, "hide": [[second]]}));
    assert!(one["scale"]["factor"].as_f64().unwrap() > both["scale"]["factor"].as_f64().unwrap(), "{one} {both}");
    assert_eq!(one["scale"]["fitted"], json!(true));
    assert_eq!(saved(&s, id).settings.hide.as_deref(), Some(&[vec![second]][..]));
}

/// A projected view follows what its parent shows, naming it, and is among the followers of the
/// edit that changes it; then it sets its own, and the parent's next change leaves it alone.
#[test]
fn a_projected_view_follows_what_its_parent_shows_until_it_sets_its_own() {
    let (mut s, view, [first, second]) = two_cubes_drawn();
    let (_, side) = create(&mut s, PROJECTED, json!({"parent": ids::encode(view), "side": "above", "place": {"at": [150, 250]}}));
    let reply = edit_view(&mut s, view, json!({"hide": [[second]]}));
    assert_eq!(reply["followers"], json!([ids::encode(side)]), "{reply}");
    assert!((width(&mut s, side) - 10.0).abs() < 1e-6);
    let inspected = inspect_view(&mut s, side);
    assert_eq!(
        inspected["settings"]["hide"],
        json!({"value": [[second]], "from": "parent", "parent": ids::encode(view), "set": null}),
        "{inspected}"
    );

    edit_view(&mut s, side, json!({"hide": []}));
    assert!((width(&mut s, side) - 40.0).abs() < 1e-6, "an empty list hides nothing, whatever the parent hides");
    let reply = edit_view(&mut s, view, json!({"hide": [[first]]}));
    assert_eq!(reply["followers"], json!([]), "{reply}");
    assert!((width(&mut s, side) - 40.0).abs() < 1e-6);
    // A display link is its own switch: `linked` doesn't touch what a view shows.
    edit_view(&mut s, side, json!({"linked": true}));
    assert!((width(&mut s, side) - 40.0).abs() < 1e-6);
}

/// A view copied alone follows nothing, so it keeps what it showed: its parent's choice becomes its
/// own, and the parent's next change leaves it alone. A view copied with its parent still follows.
#[test]
fn a_copied_view_keeps_what_it_shows() {
    let (mut s, view, [first, second]) = two_cubes_drawn();
    let (_, side) = create(&mut s, PROJECTED, json!({"parent": ids::encode(view), "side": "above"}));
    edit_view(&mut s, view, json!({"hide": [[second]]}));
    let alone = listed(&to_sheet(&mut s, COPY, json!({"view": ids::encode(side), "sheet": "Sheet 1", "position_mm": [300, 250]})))[0];
    assert!(matches!(saved(&s, alone).kind, ViewKind::Base { .. }));
    assert_eq!(saved(&s, alone).settings.hide.as_deref(), Some(&[vec![second]][..]));
    assert!((width(&mut s, alone) - 10.0).abs() < 1e-6);
    let together = listed(&to_sheet(&mut s, COPY, json!({"view": ids::encode(view), "sheet": "Sheet 1", "with_linked": true})));
    let [copy, its_side] = together[..] else { panic!("{together:?}") };
    edit_view(&mut s, view, json!({"hide": [[first]]}));
    assert_eq!(saved(&s, alone).settings.hide.as_deref(), Some(&[vec![second]][..]), "the copy follows nothing");
    let inspected = inspect_view(&mut s, its_side);
    assert_eq!(inspected["settings"]["hide"]["parent"], json!(ids::encode(copy)), "{inspected}");
}

/// Deleting an occurrence never refuses the drawing. A `hide` path to it goes quiet, a `source` path
/// to it with another left draws the other, and one with none left is a placeholder that says why;
/// undoing the delete brings the lines back. The file still reads with the paths in it.
#[test]
fn a_deleted_occurrence_leaves_the_views_that_named_it() {
    let (mut s, hides, [first, second]) = two_cubes_drawn();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let drawing = ids::encode(s.open_drawing.unwrap());
    edit_view(&mut s, hides, json!({"hide": [[second]]}));
    let (_, on_both) = create(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "source": {"occurrences": [[first], [second]]}}));
    let (_, on_second) = create(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "source": {"occurrences": [[second]]}}));
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "selection.delete", json!({"occurrences": [second]}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert!((width(&mut s, hides) - 10.0).abs() < 1e-6);
    assert!((width(&mut s, on_both) - 10.0).abs() < 1e-6);
    let gone = inspect_view(&mut s, on_second);
    assert_eq!((gone["extents"]["status"].as_str(), gone["extents"]["provenance"].as_str()), (Some("placeholder"), Some("nothing_chosen")), "{gone}");
    assert_eq!(gone["settings"]["occurrences"]["value"], json!([[second]]), "the path stays, for an undo: {gone}");
    let d = s.drawing("test").unwrap();
    assert_eq!(&Drawing::from_json(&d.to_json().unwrap()).unwrap(), &**d);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert!((width(&mut s, on_second) - 10.0).abs() < 1e-6);
}

/// The two cubes of [`two_cubes`] with an exploded view, Apart, that moves the second 30 mm further
/// along x, and a drawing of them, open, with an assembled base view at 1:1 centred at (150, 150):
/// the session, the view, the two occurrences and Apart's id.
fn exploded_pair() -> (Session, u64, [u64; 2], u64) {
    let (mut s, [first, second]) = two_cubes();
    let made = run(&mut s, "explode.create", json!({"name": "Apart", "moves": [{"occurrence": second, "translate": [30, 0, 0]}]}));
    let apart = made["view"].as_u64().unwrap();
    run(&mut s, "drawing.create", json!({}));
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let (_, view) = create(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "position_mm": [150, 150]}));
    (s, view, [first, second], apart)
}

/// The first cube's bottom front edge, through its occurrence, as dimensions, balloons and notes
/// pick it.
fn first_cubes_edge(first: u64) -> Value {
    json!({"point": [5, 0, 0], "path": [first]})
}

/// Two views of one assembly, one exploded and one not, draw differently: the cubes 10 mm apart
/// are 40 mm across, and 70 mm with the second moved out. Inspect says which each draws and who set
/// it, and both come back the same after saving and opening the design (#13048).
#[test]
fn two_views_one_exploded_draw_differently_and_both_come_back() {
    let (mut s, assembled, _, apart) = exploded_pair();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let (reply, exploded) = create(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "position_mm": [150, 250], "exploded": "Apart"}));
    assert!((width(&mut s, assembled) - 40.0).abs() < 1e-6);
    assert!((width(&mut s, exploded) - 70.0).abs() < 1e-6, "{reply}");
    let inspected = inspect_view(&mut s, exploded);
    assert_eq!(inspected["settings"]["exploded"], json!({"value": {"view": apart}, "from": "this", "set": {"view": apart}}), "{inspected}");
    let inspected = inspect_view(&mut s, assembled);
    assert_eq!(inspected["settings"]["exploded"], json!({"value": "assembled", "from": "default", "set": null}), "{inspected}");

    let folder = crate::tests::Folder::new("exploded-views");
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "file.save", json!({"path": folder.path("pair.solidcraft")}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": folder.path("pair.solidcraft")}));
    assert_eq!(opened.doc.drawings, s.doc.drawings);
    run(&mut opened, "drawing.open", json!({"drawing": drawing}));
    assert!((width(&mut opened, assembled) - 40.0).abs() < 1e-6);
    assert!((width(&mut opened, exploded) - 70.0).abs() < 1e-6);
}

/// Drawing a view exploded changes nothing in the design: the occurrences keep their placements,
/// nothing is left moved in the session, and the parts list keeps its rows (#13048).
#[test]
fn an_exploded_view_leaves_the_design_and_its_parts_list_rows() {
    let (mut s, view, _, apart) = exploded_pair();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    run(&mut s, "drawing.table.add", json!({"sheet": sheet, "kind": "parts_list"}));
    let (placed, rows) = (s.doc.occurrences.clone(), run(&mut s, "drawing.inspect", json!({}))["parts_lists"].clone());
    assert!(rows.as_array().is_some_and(|lists| !lists.is_empty()), "{rows}");
    let steps = s.undo.len();
    let reply = edit_view(&mut s, view, json!({"exploded": apart}));
    assert_eq!((reply["changed"].clone(), s.undo.len()), (json!(["exploded"]), steps + 1));
    assert!((width(&mut s, view) - 70.0).abs() < 1e-6);
    // The sheet drawn as a command reads it, as well as the view.
    run(&mut s, "drawing.scene", json!({"sheet": sheet}));
    assert_eq!(s.doc.occurrences, placed);
    assert!(s.pending_moves.is_empty() && s.exploded.is_empty());
    assert_eq!(run(&mut s, "drawing.inspect", json!({}))["parts_lists"], rows);
    run(&mut s, "edit.undo", json!({}));
    assert!((width(&mut s, view) - 40.0).abs() < 1e-6);
}

/// A projected view draws its parent's exploded view, naming it, and is among the edit's followers;
/// then it sets `assembled`, and draws assembled whatever its parent draws.
#[test]
fn a_projected_view_draws_its_parents_exploded_view_until_it_sets_its_own() {
    let (mut s, view, _, apart) = exploded_pair();
    let (_, above) = create(&mut s, PROJECTED, json!({"parent": ids::encode(view), "side": "above", "place": {"at": [150, 250]}}));
    let reply = edit_view(&mut s, view, json!({"exploded": "Apart"}));
    assert_eq!(reply["followers"], json!([ids::encode(above)]), "{reply}");
    assert!((width(&mut s, above) - 70.0).abs() < 1e-6);
    let inspected = inspect_view(&mut s, above);
    assert_eq!(
        inspected["settings"]["exploded"],
        json!({"value": {"view": apart}, "from": "parent", "parent": ids::encode(view), "set": null}),
        "{inspected}"
    );
    edit_view(&mut s, above, json!({"exploded": "assembled"}));
    assert!((width(&mut s, above) - 40.0).abs() < 1e-6, "assembled is a choice of its own");
    edit_view(&mut s, above, json!({"exploded": null}));
    assert!((width(&mut s, above) - 70.0).abs() < 1e-6, "null follows the parent again");
}

/// Whether a dimension's `reply` reads ok, near `want`.
fn near_value(reply: &Value, want: f64) -> bool {
    reply["status"] == "ok" && reply["value"].as_f64().is_some_and(|v| (v - want).abs() < 1e-6)
}

/// Whether `reply` reads ok with its arrow ending on `p`.
fn ends_near(reply: &Value, p: [f64; 3]) -> bool {
    let point = reply["point"].as_array().map(|p| p.iter().filter_map(Value::as_f64).collect::<Vec<_>>()).unwrap_or_default();
    reply["status"] == "ok" && point.len() == 3 && point.iter().zip(p).all(|(a, b)| (a - b).abs() < 1e-6)
}

/// Dimensions, balloons and notes on a view's model read where the view draws its parts (#13048), so a
/// view holding them can be exploded, itself or through a view that follows it, and they follow it. From
/// the first cube's left corner to the second's right one reads 40 assembled and 40 + 30 = 70 with the
/// second moved out; a balloon and a note on the second's bottom front edge end on its middle, (35, 0, 0)
/// assembled and (65, 0, 0) exploded. New ones go on the exploded view, and a note's branches can be
/// edited there.
#[test]
fn an_exploded_view_takes_what_measures_its_parts_where_it_draws_them() {
    let (mut s, view, [first, second], apart) = exploded_pair();
    let on_view = |view: u64| ids::encode(view);
    let across =
        json!({"linear": {"a": {"point": [0, 0, 0], "path": [first]}, "b": {"point": [40, 0, 0], "path": [second]}, "direction": "horizontal"}});
    let edge = json!({"point": [35, 0, 0], "path": [second]});
    let dimension = run(&mut s, "drawing.dimension.add", json!({"view": on_view(view), "kind": across}))["dimension"].clone();
    let balloon = run(&mut s, "drawing.balloon.add", json!({"view": on_view(view), "edge": edge, "at": [60, 40]}))["balloon"].clone();
    let note = json!({"anchor": {"edge": {"view": on_view(view), "edge": edge}}, "at": [60, 40], "content": "Deburr"});
    let note = run(&mut s, "drawing.note.add", note)["note"].clone();
    let branched = json!({"anchor": {"view": on_view(view)}, "at": [60, -40], "content": "Both", "branches": [{"edge": edge}]});
    run(&mut s, "drawing.note.add", branched.clone());
    assert!(near_value(&run(&mut s, "drawing.dimension.inspect", json!({"dimension": dimension})), 40.0));

    edit_view(&mut s, view, json!({"exploded": apart}));
    assert!((width(&mut s, view) - 70.0).abs() < 1e-6);
    let read = run(&mut s, "drawing.dimension.inspect", json!({"dimension": dimension}));
    assert!(near_value(&read, 70.0), "{read}");
    let read = run(&mut s, "drawing.balloon.inspect", json!({"balloon": balloon}));
    assert!(ends_near(&read, [65.0, 0.0, 0.0]), "{read}");
    let read = run(&mut s, "drawing.note.inspect", json!({"note": note}));
    assert!(ends_near(&read, [65.0, 0.0, 0.0]), "{read}");

    // New ones on the exploded view.
    let length = run(&mut s, "drawing.dimension.add", json!({"view": on_view(view), "kind": {"length": {"edge": edge}}}));
    assert!(near_value(&length, 10.0), "{length}");
    let added = run(&mut s, "drawing.balloon.add", json!({"view": on_view(view), "edge": edge, "at": [-60, 40]}));
    assert!(ends_near(&added, [65.0, 0.0, 0.0]), "{added}");
    run(
        &mut s,
        "drawing.note.add",
        json!({"anchor": {"edge": {"view": on_view(view), "edge": first_cubes_edge(first)}}, "at": [60, 40], "content": "A"}),
    );
    run(&mut s, "drawing.note.add", branched);
    let plain = run(&mut s, "drawing.note.add", json!({"anchor": {"view": on_view(view)}, "at": [0, -40], "content": "Exploded"}))["note"].clone();
    let add = json!({"note": plain, "set": {"branches": {"remove": [], "add": [{"edge": first_cubes_edge(first)}]}}});
    assert_eq!(run(&mut s, "drawing.note.edit", add)["changed"], json!(["branches"]));

    // Through a follower: the view above, holding a dimension of its own, follows its parent's explode.
    edit_view(&mut s, view, json!({"exploded": "assembled"}));
    let (_, above) = create(&mut s, PROJECTED, json!({"parent": on_view(view), "side": "above", "place": {"at": [150, 250]}}));
    let on_above = run(&mut s, "drawing.dimension.add", json!({"view": on_view(above), "kind": across}))["dimension"].clone();
    assert!(near_value(&run(&mut s, "drawing.dimension.inspect", json!({"dimension": on_above})), 40.0));
    edit_view(&mut s, view, json!({"exploded": apart}));
    assert!((width(&mut s, view) - 70.0).abs() < 1e-6 && (width(&mut s, above) - 70.0).abs() < 1e-6);
    assert!(near_value(&run(&mut s, "drawing.dimension.inspect", json!({"dimension": on_above})), 70.0));
}

/// A balloon and a note are taken off a drawing by their own commands (#22193): each reply names
/// what went, the other stays, a second delete is refused, and one undo puts it back.
#[test]
fn a_balloon_and_a_note_are_deleted_by_their_own_commands() {
    let (mut s, view, [_, second], _) = exploded_pair();
    let edge = json!({"point": [35, 0, 0], "path": [second]});
    let balloon = run(&mut s, "drawing.balloon.add", json!({"view": ids::encode(view), "edge": edge, "at": [60, 40]}))["balloon"].clone();
    let note = json!({"anchor": {"view": ids::encode(view)}, "at": [60, -40], "content": "Deburr"});
    let note = run(&mut s, "drawing.note.add", note)["note"].clone();

    let gone = run(&mut s, "drawing.balloon.delete", json!({"balloon": balloon}));
    assert_eq!(gone, json!({"balloon": balloon}));
    assert!(s.execute("drawing.balloon.inspect", &json!({"balloon": balloon})).is_err(), "the balloon is gone");
    assert!(s.execute("drawing.balloon.delete", &json!({"balloon": balloon})).is_err(), "a second delete is refused");
    assert_eq!(run(&mut s, "drawing.note.inspect", json!({"note": note}))["status"], "ok", "the note stays");

    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "drawing.balloon.inspect", json!({"balloon": balloon}))["status"], "ok", "one undo brings it back");

    let gone = run(&mut s, "drawing.note.delete", json!({"note": note}));
    assert_eq!(gone, json!({"note": note}));
    assert!(s.execute("drawing.note.inspect", &json!({"note": note})).is_err(), "the note is gone");
    assert!(s.execute("drawing.note.delete", &json!({"note": note})).is_err(), "a second delete is refused");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "drawing.note.inspect", json!({"note": note}))["status"], "ok", "one undo brings it back");
}

/// A projected view's centre annotation blocks its parent from exploding until the follower
/// explicitly chooses the assembled design for itself.
#[test]
fn a_centre_line_on_a_follower_blocks_an_exploded_parent() {
    let (mut s, parent, [first, _], apart) = exploded_pair();
    let (_, above) = create(&mut s, PROJECTED, json!({"parent": ids::encode(parent), "side": "above", "place": {"at": [150, 250]}}));
    let line = run(
        &mut s,
        "drawing.center_line.add",
        json!({"view": ids::encode(above), "a": {"point": [0, 0, 0], "path": [first]}, "b": {"point": [10, 0, 0], "path": [first]}, "overshoot_mm": 2.0}),
    )["line"]
        .clone();
    let (record, steps) = (s.drawing("test").unwrap().clone(), s.undo.len());
    let error = edit_refused(&mut s, parent, json!({"exploded": apart}));
    assert!(error.contains("exploded view with centre annotations"), "{error}");
    assert_eq!((s.drawing("test").unwrap().clone(), s.undo.len()), (record, steps));
    edit_view(&mut s, above, json!({"exploded": "assembled"}));
    edit_view(&mut s, parent, json!({"exploded": apart}));
    assert_eq!(run(&mut s, "drawing.center_line.inspect", json!({"line": line}))["status"], "ok");
}

/// A projected view's automatic centre-mark rule blocks its parent from exploding, and a view that
/// draws an exploded view through its parent can't turn the rule on, until the follower explicitly
/// chooses the assembled design for itself; turning the rule off is never refused.
#[test]
fn an_automatic_centre_mark_rule_on_a_follower_blocks_an_exploded_parent() {
    let (mut s, parent, _, apart) = exploded_pair();
    let (_, above) = create(&mut s, PROJECTED, json!({"parent": ids::encode(parent), "side": "above", "place": {"at": [150, 250]}}));
    let set = |s: &mut Session, view: u64, enabled: bool| {
        s.execute("drawing.auto_center_mark.set", &json!({"view": ids::encode(view), "enabled": enabled}))
    };
    set(&mut s, above, true).unwrap();
    let (record, steps) = (s.drawing("test").unwrap().clone(), s.undo.len());
    let error = edit_refused(&mut s, parent, json!({"exploded": apart}));
    assert!(error.contains("automatic centre marks enabled"), "{error}");
    assert_eq!((s.drawing("test").unwrap().clone(), s.undo.len()), (record, steps));
    edit_view(&mut s, above, json!({"exploded": "assembled"}));
    edit_view(&mut s, parent, json!({"exploded": apart}));
    assert!((width(&mut s, parent) - 70.0).abs() < 1e-6 && (width(&mut s, above) - 40.0).abs() < 1e-6);

    // Following the exploded parent again would draw the rule's view exploded: refused.
    let error = edit_refused(&mut s, above, json!({"exploded": null}));
    assert!(error.contains("automatic centre marks enabled"), "{error}");
    // The parent now draws exploded: a rule can't be enabled on it, but a follower's can be disabled.
    let error = set(&mut s, parent, true).unwrap_err().to_string();
    assert!(error.contains("automatic centre marks on an exploded view"), "{error}");
    assert_eq!(set(&mut s, above, false).unwrap()["enabled"], false);
    edit_view(&mut s, above, json!({"exploded": null}));
    assert!((width(&mut s, above) - 70.0).abs() < 1e-6);
}

/// A shaded view's pictures are rendered from the design as it places its parts, so an exploded view
/// can't be shaded yet. Whichever setting would make one, the exploded choice, a style or following a
/// shaded parent, it is refused and changes nothing, on the view itself or on a view that follows it;
/// a follower that draws assembled is no hindrance (#13048).
#[test]
fn an_exploded_view_is_never_shaded() {
    let (mut s, view, _, apart) = exploded_pair();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let unchanged = |s: &mut Session, view: u64, set: Value| {
        let (record, steps) = (s.drawing("test").unwrap().clone(), s.undo.len());
        let e = edit_refused(s, view, set);
        assert!(e.contains("not supported yet: a shaded view of an exploded view"), "{e}");
        assert_eq!((s.drawing("test").unwrap().clone(), s.undo.len()), (record, steps));
    };
    let e = create_refused(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "visual_style": "shaded", "exploded": "Apart"}));
    assert!(e.contains("not supported yet: a shaded view of an exploded view"), "{e}");

    // Exploding a shaded view, and shading an exploded one.
    edit_view(&mut s, view, json!({"visual_style": "shaded"}));
    unchanged(&mut s, view, json!({"exploded": apart}));
    edit_view(&mut s, view, json!({"visual_style": "visible", "exploded": apart}));
    unchanged(&mut s, view, json!({"visual_style": "shaded"}));
    unchanged(&mut s, view, json!({"reference_style": "shaded"}));

    // Through a follower: a shaded one blocks its parent's edit, until it draws assembled itself.
    edit_view(&mut s, view, json!({"exploded": "assembled"}));
    let (_, above) = create(&mut s, PROJECTED, json!({"parent": ids::encode(view), "side": "above", "place": {"at": [150, 250]}}));
    edit_view(&mut s, above, json!({"visual_style": "shaded"}));
    unchanged(&mut s, view, json!({"exploded": apart}));
    edit_view(&mut s, above, json!({"exploded": "assembled"}));
    edit_view(&mut s, view, json!({"exploded": apart}));

    // An exploded follower of a shaded parent can't follow its styles.
    edit_view(&mut s, view, json!({"exploded": "assembled", "visual_style": "shaded"}));
    edit_view(&mut s, above, json!({"visual_style": "visible", "exploded": apart}));
    unchanged(&mut s, above, json!({"linked": true}));
    assert!((width(&mut s, above) - 70.0).abs() < 1e-6);
}

/// Deleting an exploded view never refuses the drawing: a view that named it is a placeholder that
/// says why, its choice stays for an undo, and undoing the delete draws it exploded again.
#[test]
fn a_deleted_exploded_view_leaves_the_views_that_named_it() {
    let (mut s, view, _, apart) = exploded_pair();
    edit_view(&mut s, view, json!({"exploded": apart}));
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "explode.delete", json!({"view": apart}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let gone = inspect_view(&mut s, view);
    assert_eq!(
        (gone["extents"]["status"].as_str(), gone["extents"]["provenance"].as_str()),
        (Some("placeholder"), Some("no_exploded_view")),
        "{gone}"
    );
    assert_eq!(gone["settings"]["exploded"]["value"], json!({"view": apart}), "the choice stays, for an undo: {gone}");
    // The sheet is drawn too, the view as its placeholder.
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let scene = run(&mut s, "drawing.scene", json!({"sheet": sheet}));
    let drawn = scene["views"].as_array().and_then(|views| views.iter().find(|v| v["view"] == json!(ids::encode(view))));
    assert_eq!(drawn.map(|v| v["status"].clone()), Some(json!("placeholder")), "{scene}");
    let d = s.drawing("test").unwrap();
    assert_eq!(&Drawing::from_json(&d.to_json().unwrap()).unwrap(), &**d);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert!((width(&mut s, view) - 70.0).abs() < 1e-6);
}

/// Making or editing a view refuses an exploded choice that isn't one, with its reason, and leaves
/// the drawing and the undo steps as they were.
#[test]
fn hostile_exploded_choices_are_refused() {
    let (mut s, view, _, _) = exploded_pair();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let cases = [
        (json!("Nowhere"), "no exploded view"),
        (json!(999_999), "no exploded view"),
        (json!(-1), "exploded"),
        (json!(1e300), "exploded"),
        (json!(f64::NAN.to_string()), "no exploded view"),
        (json!([]), "exploded"),
        (json!({"view": "Apart"}), "exploded"),
        (json!(""), "exploded"),
        (json!("x".repeat(100_000)), "exploded"),
    ];
    for (choice, says) in cases {
        let e = create_refused(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "exploded": choice}));
        assert!(e.contains(says), "{choice}: {e}");
        let (record, steps) = (s.drawing("test").unwrap().clone(), s.undo.len());
        let e = edit_refused(&mut s, view, json!({"exploded": choice}));
        assert!(e.contains(says), "{choice}: {e}");
        assert_eq!((s.drawing("test").unwrap().clone(), s.undo.len()), (record, steps), "{choice}");
    }
}

/// [`configured_block`], whose length is parameter `L` (100 mm), with configuration rows Default (as
/// it is) and Long (`L` = 120 mm), drawn as [`three_views`]: the session, the views and Long's row
/// id. The front view, at 1:2, is `L` / 2 wide on paper.
fn configured_views() -> (Session, [u64; 3], u64) {
    let (s, long) = configured_block();
    let (s, views) = three_views_of(s);
    (s, views, long)
}

/// How view `view` of the drawing open stands in its sheet's scene: its status, and why (null when it
/// says nothing).
fn scene_entry(s: &mut Session, view: u64) -> (Value, Value) {
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let scene = run(s, "drawing.scene", json!({"sheet": sheet}));
    let entry = scene["views"].as_array().and_then(|views| views.iter().find(|v| v["view"] == json!(ids::encode(view)))).cloned();
    entry.map(|v| (v["status"].clone(), v.get("reason").cloned().unwrap_or(Value::Null))).unwrap_or_default()
}

/// The words a view of a row the design no longer has says.
const NO_ROW: &str = "the configuration it draws is no longer in the design";

/// A view of a row of the configuration table draws the row while the design stays as it is: the
/// front view of Long is 60 mm wide at 1:2 beside a view of the active configuration at 50, and
/// drawing it changes nothing in the design. Activating Long in the design leaves the Long view as it
/// was, and the view of the active configuration follows (#13048).
#[test]
fn a_view_of_a_row_draws_it_and_leaves_the_design() {
    let (mut s, [front, ..], _) = configured_views();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let p = json!({"sheet": sheet, "view": "front", "scale": "1:2", "position_mm": [200, 80], "configuration": "active"});
    let (_, active) = create(&mut s, BASE, p);
    let design = |s: &Session| (s.doc.params.clone(), s.doc.configs.clone(), s.doc.features.clone());
    let before = design(&s);
    let steps = s.undo.len();
    assert_eq!(edit_view(&mut s, front, json!({"configuration": "Long"}))["changed"], json!(["configuration"]));
    assert_eq!(s.undo.len(), steps + 1);
    assert!((width(&mut s, front) - 60.0).abs() < 1e-6 && (width(&mut s, active) - 50.0).abs() < 1e-6);
    run(&mut s, "drawing.scene", json!({"sheet": sheet}));
    assert!(design(&s) == before && s.pending_moves.is_empty(), "the design is as it was");
    in_the_model(&mut s, "config.activate", json!({"row": "Long"}));
    assert!((width(&mut s, front) - 60.0).abs() < 1e-6 && (width(&mut s, active) - 60.0).abs() < 1e-6);
}

/// Deleting a row never refuses the drawing: a view that drew it is a placeholder that says why, and
/// so are the views that follow it; its choice stays, for an undo, and undoing the delete draws the row
/// again, as the row comes back with its id (#13048).
#[test]
fn a_deleted_configuration_row_leaves_the_views_that_named_it() {
    let (mut s, [front, right, top], long) = configured_views();
    edit_view(&mut s, front, json!({"configuration": "Long"}));
    in_the_model(&mut s, "config.row", json!({"name": "Long", "delete": true}));
    let gone = inspect_view(&mut s, front);
    assert_eq!(
        (gone["extents"]["status"].as_str(), gone["extents"]["provenance"].as_str()),
        (Some("placeholder"), Some("no_configuration")),
        "{gone}"
    );
    assert_eq!(gone["settings"]["configuration"]["value"], json!({"row": long}), "the choice stays, for an undo: {gone}");
    assert_eq!(gone["settings"]["configuration"].get("drawn"), Some(&Value::Null), "it draws no row: {gone}");
    for view in [front, right, top] {
        assert_eq!(scene_entry(&mut s, view), (json!("placeholder"), json!(NO_ROW)), "{view}");
    }
    let d = s.drawing("test").unwrap();
    assert_eq!(&Drawing::from_json(&d.to_json().unwrap()).unwrap(), &**d);
    in_the_model(&mut s, "edit.undo", json!({}));
    assert!((width(&mut s, front) - 60.0).abs() < 1e-6);
}

/// A row added again under a deleted row's name is another row, with an id of its own: the view of the
/// deleted one stays its placeholder until it is set to the new one by name, which saves its id.
#[test]
fn a_row_added_again_by_name_is_another_row() {
    let (mut s, [front, ..], long) = configured_views();
    edit_view(&mut s, front, json!({"configuration": "Long"}));
    in_the_model(&mut s, "config.row", json!({"name": "Long", "delete": true}));
    in_the_model(&mut s, "config.row", json!({"name": "Long", "values": {"L": "120 mm"}}));
    let again = s.doc.configs.rows.iter().find(|row| row.name == "Long").map(|row| row.id).unwrap();
    assert_ne!(again, long);
    assert_eq!(inspect_view(&mut s, front)["extents"]["provenance"], json!("no_configuration"));
    edit_view(&mut s, front, json!({"configuration": "Long"}));
    assert_eq!(saved(&s, front).settings.configuration, Some(ViewConfiguration::Row(again)));
    assert!((width(&mut s, front) - 60.0).abs() < 1e-6);
}

/// A row that can't be put on the design (here a file whose Long cell is no expression) is never a
/// reason to refuse the file: the design and the drawing read, and the view is a placeholder that says
/// why, a different reason from a row that is gone (#13048).
#[test]
fn a_row_that_cant_be_put_on_the_design_is_a_placeholder() {
    let (mut s, [front, ..], long) = configured_views();
    edit_view(&mut s, front, json!({"configuration": "Long"}));
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    let row = s.doc_mut().configs.rows.iter_mut().find(|row| row.id == long).unwrap();
    row.cells[0] = "1/".into();
    assert!(solidcraft_doc::format::read(&s.doc.to_json()).is_ok());
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert_eq!(inspect_view(&mut s, front)["extents"]["provenance"], json!("not_configured"));
    assert_eq!(scene_entry(&mut s, front), (json!("placeholder"), json!("its configuration can't be applied to the design")));
    let d = s.drawing("test").unwrap();
    assert_eq!(&Drawing::from_json(&d.to_json().unwrap()).unwrap(), &**d);
}

/// A view of a row whose features fail to build draws what the row builds, as activating it would, and
/// says so: its scene reason names the row and counts the features that fail there and not in the
/// design, and inspect lists them as `config.activate` does. A view of the active row says nothing
/// (#13048).
#[test]
fn a_view_of_a_row_that_rebuilds_with_errors_draws_what_it_builds_and_says_so() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "L", "expression": "100 mm"}));
    run(&mut s, "parameters.add", json!({"name": "R", "expression": "5 mm"}));
    run(&mut s, "solid.box", json!({"length": "L", "width": 50, "height": 20, "body_name": "Block"}));
    run(&mut s, "solid.sphere", json!({"radius": "R"}));
    let sphere = s.doc.features.last().map(|f| f.name.clone()).unwrap();
    run(&mut s, "config.configure", json!({"params": ["L", "R"]}));
    run(&mut s, "config.row", json!({"name": "Broken", "values": {"L": "120 mm", "R": "-1 mm"}}));
    let broken = s.doc.configs.rows.iter().find(|row| row.name == "Broken").map(|row| row.id).unwrap();
    let (mut s, [front, _, top]) = three_views_of(s);
    edit_view(&mut s, front, json!({"configuration": "Broken"}));
    let configuration = &inspect_view(&mut s, front)["settings"]["configuration"];
    assert_eq!(configuration["drawn"], json!({"id": broken, "name": "Broken"}), "{configuration}");
    let errors = configuration["errors"].as_array().cloned().unwrap_or_default();
    assert!(errors.len() == 1 && errors[0].as_str().is_some_and(|e| e.starts_with(&format!("{sphere}: "))), "{configuration}");
    assert!((width(&mut s, front) - 60.0).abs() < 1e-6, "the block draws at the row's length");
    let says = json!("its configuration `Broken` rebuilds with 1 failing feature");
    assert_eq!(scene_entry(&mut s, front), (json!("drawn"), says.clone()));
    assert_eq!(scene_entry(&mut s, top), (json!("drawn"), says), "a view that follows it says it too");
    edit_view(&mut s, front, json!({"configuration": "active"}));
    let configuration = &inspect_view(&mut s, front)["settings"]["configuration"];
    assert_eq!((configuration["drawn"]["name"].as_str(), configuration.get("errors")), (Some("Default"), None), "{configuration}");
    assert_eq!(scene_entry(&mut s, front), (json!("drawn"), Value::Null));
    // Edited by hand, the design is in none of its rows.
    in_the_model(&mut s, "parameters.change", json!({"name": "L", "expression": "110 mm"}));
    assert_eq!(inspect_view(&mut s, front)["settings"]["configuration"].get("drawn"), Some(&Value::Null));
}

/// A shaded view's pictures are rendered from the design as it is, so a view of a configuration of its
/// own can't be shaded yet: refused, as on an exploded view, and nothing changes. Its dimensions are
/// read where it draws (#13048): the front bottom edge measures the Long row's 120 mm, not the
/// design's 100.
#[test]
fn a_view_of_a_configuration_of_its_own_is_never_shaded_and_is_dimensioned_where_it_draws() {
    let (mut s, [front, ..], _) = configured_views();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let shaded = "not supported yet: a shaded view of a configuration of its own";
    let e = create_refused(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "visual_style": "shaded", "configuration": "Long"}));
    assert!(e.contains(shaded), "{e}");
    edit_view(&mut s, front, json!({"visual_style": "shaded"}));
    let (record, steps) = (s.drawing("test").unwrap().clone(), s.undo.len());
    assert!(edit_refused(&mut s, front, json!({"configuration": "Long"})).contains(shaded));
    assert_eq!((s.drawing("test").unwrap().clone(), s.undo.len()), (record, steps));
    edit_view(&mut s, front, json!({"visual_style": "visible", "configuration": "Long"}));
    let edge = json!({"view": ids::encode(front), "kind": {"length": {"edge": {"point": [50, 0, 0], "path": []}}}});
    let added = run(&mut s, "drawing.dimension.add", edge);
    assert!(added["value"].as_f64().is_some_and(|value| (value - 120.0).abs() < 1e-9), "{added}");
}

/// A view takes its configuration by a row's name or id, or "active", and saves the row's id, so a
/// rename leaves it on its row. A projected view follows its parent's until it sets its own, and
/// inspect says where each comes from; `null` follows the parent again. Choosing what it already
/// draws changes nothing.
#[test]
fn a_view_takes_a_configuration_by_name_or_id_and_saves_the_rows_id() {
    let (mut s, [front, right, top], long) = configured_views();
    let steps = s.undo.len();
    let edited = edit_view(&mut s, front, json!({"configuration": "Long"}));
    assert_eq!(edited["changed"], json!(["configuration"]), "{edited}");
    let mut followers: Vec<Value> = edited["followers"].as_array().cloned().unwrap_or_default();
    followers.sort_by_key(|f| f.to_string());
    let mut expected = vec![json!(ids::encode(right)), json!(ids::encode(top))];
    expected.sort_by_key(|f| f.to_string());
    assert_eq!(followers, expected, "{edited}");
    assert_eq!(s.undo.len(), steps + 1);
    assert_eq!(saved(&s, front).settings.configuration, Some(ViewConfiguration::Row(long)));
    // What each draws: the row by its id and name, or the design's active row while it matches it.
    let followed = |name: &str| {
        let drawn = json!({"id": long, "name": name});
        json!({"value": {"row": long}, "from": "parent", "parent": ids::encode(front), "set": null, "drawn": drawn})
    };
    let default = s.doc.configs.rows.iter().find(|row| row.name == "Default").map(|row| row.id).unwrap();
    let active = json!({"id": default, "name": "Default"});
    assert_eq!(inspect_view(&mut s, top)["settings"]["configuration"], followed("Long"));
    // Renamed in the design, the view stays on its row.
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "config.row", json!({"name": "Long", "rename": "Longer"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert_eq!(saved(&s, front).settings.configuration, Some(ViewConfiguration::Row(long)));
    assert!((width(&mut s, front) - 60.0).abs() < 1e-6, "it draws the renamed row");
    // By id it is the same row: nothing changes and no step is made.
    let steps = s.undo.len();
    assert_eq!(edit_view(&mut s, front, json!({"configuration": long}))["changed"], json!([]));
    assert_eq!(s.undo.len(), steps);
    // "active" is a choice of its own under a parent that draws another row.
    edit_view(&mut s, right, json!({"configuration": "ACTIVE"}));
    let own = json!({"value": "active", "from": "parent", "parent": ids::encode(right), "set": null, "drawn": active.clone()});
    assert_eq!(inspect_view(&mut s, top)["settings"]["configuration"], own);
    edit_view(&mut s, right, json!({"configuration": null}));
    assert_eq!(inspect_view(&mut s, top)["settings"]["configuration"], followed("Longer"), "null follows the parent again");
    edit_view(&mut s, front, json!({"configuration": null}));
    let base = json!({"value": "active", "from": "default", "set": null, "drawn": active});
    assert_eq!(inspect_view(&mut s, front)["settings"]["configuration"], base);
    // A base view made with a row saves its id.
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let (_, made) = create(&mut s, BASE, json!({"sheet": sheet, "scale": "1:2", "position_mm": [200, 80], "configuration": "Longer"}));
    assert_eq!(saved(&s, made).settings.configuration, Some(ViewConfiguration::Row(long)));
}

/// Making or editing a view refuses a configuration the design hasn't got, or that isn't one, with its
/// reason, and leaves the drawing and the undo steps as they were. A row is named by its name (text)
/// or its id (a whole number), never by the form a view saves it in.
#[test]
fn hostile_configuration_choices_are_refused() {
    let (mut s, [front, ..], long) = configured_views();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let cases = [
        (json!("Tall"), "no configuration `Tall`"),
        (json!(long.to_string()), "no configuration"),
        (json!(999_999), "no configuration `999999`"),
        (json!(-1), "configuration"),
        (json!(1.5), "configuration"),
        (json!(1e300), "configuration"),
        (json!([]), "configuration"),
        (json!({"row": long}), "configuration"),
        (json!(""), "no configuration"),
        (json!("x".repeat(100_000)), "no configuration"),
    ];
    for (choice, says) in cases {
        let e = create_refused(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "configuration": choice}));
        assert!(e.contains(says), "{choice}: {}", e.chars().take(200).collect::<String>());
        let (record, steps) = (s.drawing("test").unwrap().clone(), s.undo.len());
        let e = edit_refused(&mut s, front, json!({"configuration": choice}));
        assert!(e.contains(says), "{choice}: {}", e.chars().take(200).collect::<String>());
        assert_eq!((s.drawing("test").unwrap().clone(), s.undo.len()), (record, steps), "{choice}");
    }
    // A design with no configuration table has no row to name.
    let (mut plain, [view, ..]) = three_views();
    assert!(edit_refused(&mut plain, view, json!({"configuration": "Long"})).contains("no configuration `Long`"));
    edit_view(&mut plain, view, json!({"configuration": "active"}));
    // A deleted row is refused by its id, while the view that saved it still reads.
    edit_view(&mut s, front, json!({"configuration": long}));
    in_the_model(&mut s, "config.row", json!({"name": "Long", "delete": true}));
    assert!(edit_refused(&mut s, front, json!({"configuration": long})).contains(&format!("no configuration `{long}`")));
    assert_eq!(saved(&s, front).settings.configuration, Some(ViewConfiguration::Row(long)));
}

/// A view's height on its sheet: [`hinge_pair`]'s top view is 40 mm with the leaf flat, 20 mm with it
/// turned up square.
fn height(s: &mut Session, id: u64) -> f64 {
    boundary(&inspect_view(s, id)).0.y
}

/// Two views of one hinge, one drawing the design's placement and one with the leaf turned up, draw
/// differently. Inspect says what each sets and who set it, the saved values in radians and what it
/// draws in degrees, and both come back the same after saving and opening the design (#13048).
#[test]
fn two_views_one_posed_draw_differently_and_both_come_back() {
    let (mut s, designed, hinge) = hinge_pair();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let pose = json!([{"joint": "Hinge", "values": [90]}]);
    let (reply, posed) = create(&mut s, BASE, json!({"sheet": sheet, "view": "top", "scale": "1:1", "position_mm": [150, 250], "pose": pose}));
    assert!((height(&mut s, designed) - 40.0).abs() < 1e-6);
    assert!((height(&mut s, posed) - 20.0).abs() < 1e-6, "{reply}");
    let saved = json!({"joints": [{"joint": hinge, "values": [std::f64::consts::FRAC_PI_2]}]});
    let inspected = inspect_view(&mut s, posed);
    assert_eq!(inspected["settings"]["pose"]["value"], saved, "{inspected}");
    assert_eq!((&inspected["settings"]["pose"]["from"], &inspected["settings"]["pose"]["set"]), (&json!("this"), &saved), "{inspected}");
    assert_eq!(inspected["settings"]["pose"]["drawn"], json!([{"joint": hinge, "name": "Hinge", "values": [90.0]}]), "{inspected}");
    let inspected = inspect_view(&mut s, designed);
    assert_eq!(inspected["settings"]["pose"], json!({"value": "designed", "from": "default", "set": null}), "{inspected}");

    let folder = crate::tests::Folder::new("posed-views");
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "file.save", json!({"path": folder.path("hinge.solidcraft")}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": folder.path("hinge.solidcraft")}));
    assert_eq!(opened.doc.drawings, s.doc.drawings);
    run(&mut opened, "drawing.open", json!({"drawing": drawing}));
    assert!((height(&mut opened, designed) - 40.0).abs() < 1e-6);
    assert!((height(&mut opened, posed) - 20.0).abs() < 1e-6);
}

/// Drawing a view in a position of its own changes nothing in the design: the joint keeps its value,
/// the occurrences their placements, and nothing is left moved. The edit is one undo step.
#[test]
fn a_posed_view_leaves_the_design() {
    let (mut s, view, _) = hinge_pair();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let (joints, placed) = (s.doc.assembly.joints.clone(), s.doc.occurrences.clone());
    let steps = s.undo.len();
    let reply = edit_view(&mut s, view, json!({"pose": [{"joint": "Hinge", "values": [90]}]}));
    assert_eq!((reply["changed"].clone(), s.undo.len()), (json!(["pose"]), steps + 1));
    assert!((height(&mut s, view) - 20.0).abs() < 1e-6);
    run(&mut s, "drawing.scene", json!({"sheet": sheet}));
    assert_eq!((&s.doc.assembly.joints, &s.doc.occurrences), (&joints, &placed));
    assert!(s.pending_moves.is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert!((height(&mut s, view) - 40.0).abs() < 1e-6);
}

/// A projected view draws its parent's position until it sets its own: `designed` draws the design's
/// placement whatever its parent draws, and null follows the parent again.
#[test]
fn a_projected_view_draws_its_parents_position_until_it_sets_its_own() {
    let (mut s, view, _) = hinge_pair();
    let (_, front) = create(&mut s, PROJECTED, json!({"parent": ids::encode(view), "side": "below", "place": {"at": [150, 60]}}));
    let depth = |s: &mut Session| height(s, front);
    let flat = depth(&mut s);
    let reply = edit_view(&mut s, view, json!({"pose": [{"joint": "Hinge", "values": [90]}]}));
    assert_eq!(reply["followers"], json!([ids::encode(front)]), "{reply}");
    let turned = depth(&mut s);
    assert!((flat - 2.0).abs() < 1e-6 && (turned - 20.0).abs() < 1e-6, "{flat} {turned}");
    assert_eq!(inspect_view(&mut s, front)["settings"]["pose"]["from"], json!("parent"));
    edit_view(&mut s, front, json!({"pose": "designed"}));
    assert!((depth(&mut s) - 2.0).abs() < 1e-6, "designed is a choice of its own");
    edit_view(&mut s, front, json!({"pose": null}));
    assert!((depth(&mut s) - 20.0).abs() < 1e-6, "null follows the parent again");
}

/// A value past its joint's limit draws at the limit, as `joint.drive` puts it, and inspect says what
/// it draws beside what is saved.
#[test]
fn a_pose_past_a_limit_is_clamped_and_said() {
    let (mut s, view, hinge) = hinge_pair();
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "joint.limits", json!({"joint": "Hinge", "min": 0, "max": 60}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    edit_view(&mut s, view, json!({"pose": [{"joint": "Hinge", "values": [90]}]}));
    // The leaf at 60°: 20 cos 60° = 10 mm beyond the plate's 20.
    assert!((height(&mut s, view) - 30.0).abs() < 1e-6);
    let pose = inspect_view(&mut s, view)["settings"]["pose"].clone();
    assert_eq!(pose["value"], json!({"joints": [{"joint": hinge, "values": [std::f64::consts::FRAC_PI_2]}]}), "saved as asked: {pose}");
    let drawn = pose["drawn"][0]["values"][0].as_f64().unwrap();
    assert!((drawn - 60.0).abs() < 1e-9, "{pose}");
}

/// A joint the design no longer has is skipped: the view draws with the rest (here none, so the design's
/// own placement, and its copy solves no joint), inspect names it, and the sheet's entry for the view
/// says how many are gone, as a drawn note says why beside its lines. Undoing the delete draws the
/// position again.
#[test]
fn a_pose_naming_a_gone_joint_skips_it() {
    let (mut s, view, hinge) = hinge_pair();
    edit_view(&mut s, view, json!({"pose": [{"joint": "Hinge", "values": [90]}]}));
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "joint.delete", json!({"joint": "Hinge"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert!((height(&mut s, view) - 40.0).abs() < 1e-6, "drawn as the design places it");
    let resolved = s.drawing("test").unwrap().resolve_view(view).unwrap();
    crate::cmd::joints::solve_counts::take();
    crate::drawing_view::copy_for(&s, &crate::drawing_view::Choice::of(&resolved)).unwrap();
    assert_eq!(crate::cmd::joints::solve_counts::take().0, 0, "no joint left to drive, none solved");
    let pose = inspect_view(&mut s, view)["settings"]["pose"].clone();
    assert_eq!((&pose["skipped"], &pose["drawn"]), (&json!([hinge]), &json!([])), "{pose}");
    // The sheet draws it too.
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let scene = run(&mut s, "drawing.scene", json!({"sheet": sheet}));
    let drawn = scene["views"].as_array().and_then(|views| views.iter().find(|v| v["view"] == json!(ids::encode(view)))).cloned();
    let entry = drawn.map(|v| (v["status"].clone(), v["reason"].clone()));
    assert_eq!(entry, Some((json!("drawn"), json!("its position names 1 joint the design no longer has"))), "{scene}");
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert!((height(&mut s, view) - 20.0).abs() < 1e-6, "the joint back, the position drawn");
    let scene = run(&mut s, "drawing.scene", json!({"sheet": sheet}));
    let drawn = scene["views"].as_array().and_then(|views| views.iter().find(|v| v["view"] == json!(ids::encode(view)))).cloned();
    assert_eq!(drawn.map(|v| v.get("reason").cloned()), Some(None), "nothing gone, nothing said: {scene}");
}

/// Makes motion study Open in [`hinge_pair`]'s design, turning Hinge along `points` ([step, degrees])
/// over `steps` steps, or makes it again with them: with the drawing closed, then open again.
fn study(s: &mut Session, steps: u32, points: Value) {
    with_the_model(s, |s| {
        let again = if s.doc.assembly.studies.is_empty() { json!({}) } else { json!({"study": "Open"}) };
        let mut p = json!({"name": "Open", "steps": steps, "keys": [{"joint": "Hinge", "points": points}]});
        p.as_object_mut().unwrap().extend(again.as_object().unwrap().clone());
        run(s, "motion.study", p);
    });
}

/// The top view's height with the leaf at `degrees`: the plate's 20 mm and the leaf's 20 cos.
fn turned(degrees: f64) -> f64 {
    20.0 + 20.0 * degrees.to_radians().cos()
}

/// A view at a step of a motion study draws the assembly as `motion.play` sets it there, and the design
/// is left as it was. Inspect says the study, the step and the values it draws, in degrees. The view
/// keeps the step, not the values, so it follows the study when the study changes (#13048).
#[test]
fn a_view_at_a_study_step_draws_as_play_sets_it_and_follows_the_study() {
    let (mut s, view, hinge) = hinge_pair();
    study(&mut s, 10, json!([[0, 0], [10, 90]]));
    let (joints, placed) = (s.doc.assembly.joints.clone(), s.doc.occurrences.clone());
    edit_view(&mut s, view, json!({"pose": {"study": "Open", "step": 10}}));
    assert!((height(&mut s, view) - turned(90.0)).abs() < 1e-6);
    edit_view(&mut s, view, json!({"pose": {"study": "Open", "step": 5}}));
    assert!((height(&mut s, view) - turned(45.0)).abs() < 1e-6);
    assert_eq!((&s.doc.assembly.joints, &s.doc.occurrences), (&joints, &placed), "the design is left as it was");
    assert!(s.pending_moves.is_empty());
    let id = s.doc.assembly.studies[0].id;
    let pose = inspect_view(&mut s, view)["settings"]["pose"].clone();
    assert_eq!(pose["value"], json!({"study": {"id": id, "step": 5}}), "{pose}");
    assert_eq!(pose["study"], json!({"id": id, "name": "Open", "step": 5}), "{pose}");
    assert_eq!((&pose["drawn"][0]["joint"], &pose["skipped"]), (&json!(hinge), &json!([])), "{pose}");
    assert!((pose["drawn"][0]["values"][0].as_f64().unwrap() - 45.0).abs() < 1e-9, "{pose}");
    // The study turned to 60° at its end: step 5 is now 30°.
    study(&mut s, 10, json!([[0, 0], [10, 60]]));
    assert!((height(&mut s, view) - turned(30.0)).abs() < 1e-6);
}

/// A saved step past the study's end, once the study is made shorter, draws the study's last step, as
/// `motion.play` holds a step there, and inspect gives the step it draws. A study the design no longer
/// has draws the design's placement, and the sheet says why, as for a gone joint.
#[test]
fn a_study_step_past_the_end_draws_the_last_and_a_gone_study_is_said() {
    let (mut s, view, _) = hinge_pair();
    study(&mut s, 10, json!([[0, 0], [10, 90]]));
    edit_view(&mut s, view, json!({"pose": {"study": "Open", "step": 10}}));
    study(&mut s, 5, json!([[0, 0], [10, 90]]));
    // The key reaches 90° at step 10; the study now ends at step 5, where it is at 45°.
    assert!((height(&mut s, view) - turned(45.0)).abs() < 1e-6);
    assert_eq!(inspect_view(&mut s, view)["settings"]["pose"]["study"]["step"], json!(5));
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "motion.delete", json!({"study": "Open"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert!((height(&mut s, view) - 40.0).abs() < 1e-6, "drawn as the design places it");
    let pose = inspect_view(&mut s, view)["settings"]["pose"].clone();
    assert_eq!((pose.get("study"), &pose["drawn"]), (None, &json!([])), "{pose}");
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let scene = run(&mut s, "drawing.scene", json!({"sheet": sheet}));
    let drawn = scene["views"].as_array().and_then(|views| views.iter().find(|v| v["view"] == json!(ids::encode(view)))).cloned();
    let entry = drawn.map(|v| (v["status"].clone(), v["reason"].clone()));
    assert_eq!(entry, Some((json!("drawn"), json!("its position names a motion study the design no longer has"))), "{scene}");
}

/// Making or editing a view refuses a study step the design can't draw, with its reason, and leaves the
/// drawing and the undo steps as they were: an unknown study, a step outside the study's timeline or
/// not a whole one, and a step that isn't one.
#[test]
fn hostile_study_steps_are_refused() {
    let (mut s, view, _) = hinge_pair();
    study(&mut s, 10, json!([[0, 0], [10, 90]]));
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let cases = [
        (json!({"study": "Nowhere", "step": 1}), "no motion study `Nowhere`"),
        (json!({"study": 999_999, "step": 1}), "no motion study `999999`"),
        (json!({"study": "Open", "step": 11}), "motion study `Open`: 0 to 10"),
        (json!({"study": "Open", "step": -1}), "0 to 10"),
        (json!({"study": "Open", "step": 2.5}), "0 to 10"),
        (json!({"study": "Open", "step": u64::MAX}), "0 to 10"),
        (json!({"study": "Open", "step": "5"}), "0 to 10"),
        (json!({"study": [1], "step": 1}), "`study` must be an id or a name"),
        (json!({"study": "Open"}), "pose"),
        (json!({"study": "Open", "step": 1, "at": 1}), "pose"),
    ];
    for (pose, says) in cases {
        let e = create_refused(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "pose": pose}));
        assert!(e.contains(says), "{pose}: {e}");
        let (record, steps) = (s.drawing("test").unwrap().clone(), s.undo.len());
        let e = edit_refused(&mut s, view, json!({"pose": pose}));
        assert!(e.contains(says), "{pose}: {e}");
        assert_eq!((s.drawing("test").unwrap().clone(), s.undo.len()), (record, steps), "{pose}");
    }
}

/// A nut on a thread: the design's own plate, 40 x 20 x 2 mm from the origin, and a 10 x 10 x 5 mm
/// nut on it in component Nut, on cylindrical joint Thread about the vertical through the nut's
/// centre, which a screw link advances 2 mm a turn. Motion study Turn keys only the thread's turn,
/// 0 to 720° over 10 steps. A drawing, open, with a front view at 1:1. The session and the view.
fn screw_nut() -> (Session, u64) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 2, "body_name": "Plate"}));
    run(&mut s, "component.create", json!({"name": "Nut", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 5, "corner": [15, 5, 2], "body_name": "Nut", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let at = json!({"occurrence": 0, "point": [20, 10, 2], "z": [0, 0, 1]});
    run(&mut s, "joint.as_built", json!({"type": "cylindrical", "name": "Thread", "a": 0, "b": "Nut", "at": at}));
    run(&mut s, "joint.screw", json!({"joint": "Thread", "pitch": 2}));
    let keys = json!([{"joint": "Thread", "index": 0, "points": [[0, 0], [10, 720]]}]);
    run(&mut s, "motion.study", json!({"name": "Turn", "steps": 10, "keys": keys}));
    run(&mut s, "drawing.create", json!({}));
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let p = json!({"sheet": sheet, "view": "front", "scale": "1:1", "position_mm": [150, 150]});
    let view = ids::decode(run(&mut s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap();
    (s, view)
}

/// Inspect gives the values a posed view draws as the solver places them: the thread at two turns,
/// the nut's travel follows its screw link, 4 mm, though the joint saves none.
#[test]
fn a_posed_view_says_the_travel_its_screw_link_draws() {
    let (mut s, view) = screw_nut();
    // The plate's 2 mm and the nut's 5 above it.
    assert!((height(&mut s, view) - 7.0).abs() < 1e-6);
    edit_view(&mut s, view, json!({"pose": [{"joint": "Thread", "values": [720, 0]}]}));
    let pose = inspect_view(&mut s, view)["settings"]["pose"].clone();
    assert!((height(&mut s, view) - 11.0).abs() < 1e-6, "{pose}");
    let values = &pose["drawn"][0]["values"];
    assert!((values[0].as_f64().unwrap_or(f64::NAN) - 720.0).abs() < 1e-6, "{pose}");
    assert!((values[1].as_f64().unwrap_or(f64::NAN) - 4.0).abs() < 1e-6, "{pose}");
}

/// A study keys only the thread's turn, and the nut's travel follows it through the screw link: two
/// turns rise two pitches, 4 mm. (A screw link sets the travel whatever the solve holds; holding only
/// the keyed values shows in a closed loop, which
/// [`a_study_holds_only_the_values_it_keys_and_a_closed_loop_moves_the_rest`] covers.)
#[test]
fn a_study_step_holds_only_the_values_the_study_keys() {
    let (mut s, view) = screw_nut();
    // The plate's 2 mm and the nut's 5 above it.
    assert!((height(&mut s, view) - 7.0).abs() < 1e-6);
    edit_view(&mut s, view, json!({"pose": {"study": "Turn", "step": 10}}));
    let pose = inspect_view(&mut s, view)["settings"]["pose"].clone();
    assert!((height(&mut s, view) - 11.0).abs() < 1e-6, "{pose}");
    let slide = pose["drawn"][0]["values"][1].as_f64().unwrap_or(f64::NAN);
    assert!((slide - 4.0).abs() < 1e-6, "{pose}");
}

/// A study holds only the values it keys, as `motion.play` does, and a closed loop moves the rest. In
/// [`slotted_arm`](crate::cmd::joints::tests::slotted_arm), study Swing keys only the slot's angle; the
/// loop slides the pin out along the slot as it turns. A view at the study's last step draws the slot
/// at the angle and slide play gives there. Holding the slide too, the loop couldn't close.
#[test]
fn a_study_holds_only_the_values_it_keys_and_a_closed_loop_moves_the_rest() {
    let mut s = crate::cmd::joints::tests::slotted_arm(false, 0.0);
    let keys = json!([{"joint": "Slot", "index": 0, "points": [[0, 0], [10, 20]]}]);
    run(&mut s, "motion.study", json!({"name": "Swing", "steps": 10, "keys": keys}));
    // Played on a copy: the view draws from the design's placement, where the slide is 26 mm.
    let mut played = s.scratch();
    run(&mut played, "motion.play", json!({"study": "Swing", "step": 10}));
    let id = |name: &str| played.doc.assembly.joints.iter().find(|j| j.name == name).map(|j| j.id).unwrap();
    let values = solidcraft_doc::joints::joint_values(&played.doc.assembly);
    let (want, yaw) = (values.get(&id("Slot")).cloned().unwrap(), values.get(&id("Yaw")).and_then(|v| v.first().copied()).unwrap());
    // Play moved the loop: the slot turned to 20° and the pin slid out to the fixture's closed form for the
    // clamp's turn, √(26² + 52² − 2·26·52 cos θ), 26.80 mm here, off the 26 mm it starts at.
    let slid = (26.0f64.powi(2) + 52.0f64.powi(2) - 2.0 * 26.0 * 52.0 * yaw.cos()).sqrt();
    assert!(
        (want[0].to_degrees().abs() - 20.0).abs() < 1e-6 && (want[1].abs() - slid).abs() < 1e-6 && (want[1].abs() - 26.0).abs() > 0.5,
        "play moved the loop: {want:?}, the clamp turned {yaw} rad"
    );
    run(&mut s, "drawing.create", json!({}));
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let p = json!({"sheet": sheet, "view": "top", "scale": "1:1", "position_mm": [150, 150]});
    let view = ids::decode(run(&mut s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap();
    edit_view(&mut s, view, json!({"pose": {"study": "Swing", "step": 10}}));
    let pose = inspect_view(&mut s, view)["settings"]["pose"].clone();
    let drawn = &pose["drawn"][0]["values"];
    let (angle, slide) = (drawn[0].as_f64().unwrap_or(f64::NAN), drawn[1].as_f64().unwrap_or(f64::NAN));
    assert!((angle - want[0].to_degrees()).abs() < 1e-6 && (slide - want[1]).abs() < 1e-6, "drawn {angle}°, {slide} mm; play {want:?}: {pose}");
}

/// Dimensions on a posed view read where it draws its parts (#13048), by the rule an exploded view
/// follows: a view holding one can be posed, and the dimension follows it, and new ones go on a posed
/// view. The plate's far top corner to the leaf's free top corner reads 40 with the leaf flat and 18
/// with it turned up 90° about the hinge line (y 20, z 0): (0, 40, 2) goes to (0, 20 - 2, 20). The
/// plate's top front edge, which doesn't move, reads 40 in both. A shaded posed view is still refused:
/// its pictures are rendered from the design's placement.
#[test]
fn a_posed_view_takes_what_measures_it_where_it_draws_it() {
    let (mut s, view, _) = hinge_pair();
    let (_, other) = {
        let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
        create(&mut s, BASE, json!({"sheet": sheet, "view": "top", "scale": "1:1", "position_mm": [150, 250]}))
    };
    let leaf = s.doc.occurrences.iter().find(|o| o.name.starts_with("Leaf")).map(|o| o.id).unwrap();
    let across = |view: u64| {
        let kind = json!({"linear": {"a": [0, 0, 2], "b": {"point": [0, 40, 2], "path": [leaf]}, "direction": "vertical"}});
        json!({"view": ids::encode(view), "kind": kind})
    };
    // The plate's top front edge.
    let edge = |view: u64| json!({"view": ids::encode(view), "kind": {"length": {"edge": {"point": [20, 0, 2], "path": []}}}});
    let free = run(&mut s, "drawing.dimension.add", across(other))["dimension"].clone();
    let front = run(&mut s, "drawing.dimension.add", edge(other))["dimension"].clone();
    let read = |s: &mut Session, d: &Value| run(s, "drawing.dimension.inspect", json!({"dimension": d}));
    assert!(near_value(&read(&mut s, &free), 40.0));
    edit_view(&mut s, other, json!({"pose": [{"joint": "Hinge", "values": [90]}]}));
    assert!(near_value(&read(&mut s, &free), 18.0) && near_value(&read(&mut s, &front), 40.0));
    edit_view(&mut s, view, json!({"pose": [{"joint": "Hinge", "values": [90]}]}));
    assert!(near_value(&run(&mut s, "drawing.dimension.add", across(view)), 18.0));
    assert!(near_value(&run(&mut s, "drawing.dimension.add", edge(view)), 40.0));
    let e = edit_refused(&mut s, other, json!({"visual_style": "shaded"}));
    assert!(e.contains("not supported yet: a shaded view in a position of its own"), "{e}");
}

/// A view exploded from a position: the leaf turned up, then moved out 30 mm along Y. Exploded alone,
/// the flat leaf moves out to 70 mm; posed alone, the view is 20 mm.
#[test]
fn an_exploded_view_of_a_position_explodes_from_it() {
    let (mut s, view, _) = hinge_pair();
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    add_a_pin(&mut s);
    let leaf = s.doc.occurrences.iter().find(|o| o.name.starts_with("Leaf")).map(|o| o.id).unwrap();
    run(&mut s, "explode.create", json!({"name": "Apart", "moves": [{"occurrence": leaf, "translate": [0, 30, 0]}]}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    edit_view(&mut s, view, json!({"exploded": "Apart"}));
    assert!((height(&mut s, view) - 70.0).abs() < 1e-6);
    edit_view(&mut s, view, json!({"pose": [{"joint": "Hinge", "values": [90]}]}));
    // The turned leaf spans y 18..20, moved out to 48..50.
    assert!((height(&mut s, view) - 50.0).abs() < 1e-6);
    edit_view(&mut s, view, json!({"exploded": "assembled"}));
    assert!((height(&mut s, view) - 20.0).abs() < 1e-6);
}

/// Making or editing a view refuses a position that isn't one, with its reason, and leaves the
/// drawing and the undo steps as they were.
#[test]
fn hostile_poses_are_refused() {
    let (mut s, view, _) = hinge_pair();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let many: Vec<Value> = (0..65).map(|_| json!({"joint": "Hinge", "values": [1]})).collect();
    let cases = [
        (json!([{"joint": "Nowhere", "values": [1]}]), "no joint"),
        (json!([{"joint": 999_999, "values": [1]}]), "no joint"),
        (json!([{"joint": "Hinge", "values": [1, 2]}]), "1 value"),
        (json!([{"joint": "Hinge", "values": []}]), "1 value"),
        // Main's joint check names the range a value must keep to before the pose's own check.
        (json!([{"joint": "Hinge", "values": [1e300]}]), "between -1e9 and 1e9"),
        (json!([{"joint": "Hinge", "values": ["1/0"]}]), "Hinge"),
        (json!([{"joint": "Hinge", "values": [1]}, {"joint": "Hinge", "values": [2]}]), "each joint once"),
        (json!(many), "at most 64 joints"),
        (json!([]), "at least one joint"),
        (json!("posed"), "pose"),
        (json!({"joint": "Hinge"}), "pose"),
        (json!([{"joint": "Hinge", "values": [1], "extra": 1}]), "pose"),
        (json!("x".repeat(100_000)), "pose"),
    ];
    for (pose, says) in cases {
        let shown = pose.to_string().chars().take(60).collect::<String>();
        let e = create_refused(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "pose": pose}));
        assert!(e.contains(says), "{shown}: {e}");
        let (record, steps) = (s.drawing("test").unwrap().clone(), s.undo.len());
        let e = edit_refused(&mut s, view, json!({"pose": pose}));
        assert!(e.contains(says), "{shown}: {e}");
        assert_eq!((s.drawing("test").unwrap().clone(), s.undo.len()), (record, steps), "{shown}");
    }
}

/// Making or editing a view refuses what the snapshot refuses, with its reason, and leaves the
/// drawing and the undo steps as they were: a path no occurrence makes, too many paths or ids, `[]`
/// in `hide`, and a choice that leaves nothing. `[]` in `source` with only placed bodies is that.
#[test]
fn hostile_source_and_hide_on_a_saved_view_are_refused() {
    let (mut s, view, [first, second]) = two_cubes_drawn();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let many: Vec<Vec<u64>> = vec![vec![first]; 1001];
    let deep: Vec<Vec<u64>> = vec![vec![first; 64]; 65];
    let cases = [
        (json!({"source": {"occurrences": [[999]]}}), "source.occurrences path"),
        (json!({"hide": [[999]]}), "hide path"),
        (json!({"hide": [[first, first]]}), "repeats an id"),
        (json!({"source": {"occurrences": [vec![first; 65]]}}), "1..64 ids"),
        (json!({"hide": many}), "at most 1000 occurrence paths"),
        (json!({"source": {"occurrences": deep}}), "4096 ids in all"),
        (json!({"hide": [[]]}), "hide [] would leave out"),
        (json!({"hide": [[first], [second]]}), NOTHING_CHOSEN),
        (json!({"source": {"occurrences": []}}), NOTHING_CHOSEN),
        (json!({"source": {"occurrences": [[]]}}), NOTHING_CHOSEN),
        (json!({"source": {"occurrences": [[first]]}, "hide": [[first]]}), NOTHING_CHOSEN),
        (json!({"hide": [[-1]]}), "hide"),
        (json!({"source": {}}), "occurrences"),
    ];
    for (set, says) in cases {
        let (record, steps) = (s.drawing("test").unwrap().clone(), s.undo.len());
        let e = edit_refused(&mut s, view, set.clone());
        assert!(e.contains(says), "edit {}: {e}", set.to_string().chars().take(60).collect::<String>());
        assert!(Arc::ptr_eq(&record, s.drawing("test").unwrap()) && s.undo.len() == steps, "edit");
        let mut p = set.clone();
        p["sheet"] = json!(sheet);
        let e = create_refused(&mut s, BASE, p);
        assert!(e.contains(says), "base {}: {e}", set.to_string().chars().take(60).collect::<String>());
    }
}

/// With nothing to measure anyway, a choice isn't what leaves it empty: the view is made and gets
/// its placeholder for the design's reason.
#[test]
fn a_choice_on_a_design_with_no_bodies_gets_the_designs_placeholder() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "drawing.create", json!({}));
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let (_, view) = create(&mut s, BASE, json!({"sheet": sheet, "source": {"occurrences": [[]]}}));
    assert_eq!(inspect_view(&mut s, view)["extents"]["provenance"], json!("no_bodies"));
}

/// Seeded choices on a view, edited one after another: each edit is refused exactly when the rule
/// worked out the long way leaves nothing; otherwise the view is as wide as the copies left.
#[test]
fn seeded_choices_on_a_saved_view_agree_with_the_rule() {
    let pick = |seed: u64, salt: u64, n: u64| ((seed ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) % n;
    let (mut s, view, [first, second]) = two_cubes_drawn();
    for seed in 1..=120u64 {
        let source: Option<Vec<Vec<u64>>> =
            [None, Some(vec![vec![first]]), Some(vec![vec![second]]), Some(vec![vec![first], vec![second]]), Some(vec![vec![]])]
                [pick(seed, 1, 5) as usize]
                .clone();
        let hide: Vec<Vec<u64>> = [vec![], vec![vec![first]], vec![vec![second]]][pick(seed, 2, 3) as usize].clone();
        let shown = |copy: u64| source.as_ref().is_none_or(|paths| paths.contains(&vec![copy])) && !hide.contains(&vec![copy]);
        let set = json!({"source": source.as_ref().map(|paths| json!({"occurrences": paths})), "hide": hide});
        let want = match (shown(first), shown(second)) {
            (true, true) => Some(40.0),
            (false, false) => None,
            _ => Some(10.0),
        };
        match want {
            Some(want) => {
                edit_view(&mut s, view, set);
                let got = width(&mut s, view);
                assert!((got - want).abs() < 1e-6, "seed {seed}: {got}");
                let d = s.drawing("test").unwrap();
                assert_eq!(&Drawing::from_json(&d.to_json().unwrap()).unwrap(), &**d, "seed {seed}");
            }
            None => assert!(edit_refused(&mut s, view, set).contains(NOTHING_CHOSEN), "seed {seed}"),
        }
    }
}

/// What a reply lists as outside its sheet's drawing space, and those views' ids.
fn outside(reply: &Value) -> (Vec<Value>, Vec<u64>) {
    let listed = reply.get("outside").map_or(Vec::new(), |o| o.as_array().unwrap().clone());
    let views = listed.iter().map(|v| ids::decode(v["view"].as_str().unwrap()).unwrap()).collect();
    (listed, views)
}

fn sheet_set(s: &mut Session, sheet: u64, mut p: Value) -> Value {
    p["sheet"] = json!(ids::encode(sheet));
    let reply = run(s, "drawing.sheet.set", p);
    let schema = output_schema_for::<crate::schema::drawing_sheet::SheetsResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    reply
}

/// A change of paper moves no view and lists those it leaves outside the drawing space (S5, #3875).
/// On A3 the three views fit; on A4, either way up, the right and top views leave it (the right
/// view reaches x 312.5, past A4's 287), and stay where they were. A border through the right
/// view's edge leaves it inside, and so does one short of it by less than a rounding; a tenth of a
/// millimetre less paper puts it outside. A change of name
/// checks nothing, undo brings the paper back, a view moved or copied to a smaller sheet is listed
/// when it leaves the drawing space there, and a view with nothing to measure is listed by its
/// placeholder, saying why.
#[test]
fn a_change_of_paper_lists_the_views_it_leaves_outside_and_moves_none() {
    let (mut s, [front, right, top]) = three_views();
    let sheet = saved(&s, front).sheet;
    let places = |s: &Session| [front, right, top].map(|v| saved(s, v).place);
    let before = places(&s);
    let (listed, views) = outside(&sheet_set(&mut s, sheet, json!({"size": {"standard": "a4"}})));
    assert_eq!(views, [top, right], "by id");
    assert_box(&listed[0], [287.5, 225.0], [312.5, 275.0]);
    assert_box(&listed[1], [287.5, 145.0], [312.5, 155.0]);
    assert_eq!(places(&s), before, "nothing moves");
    assert_eq!(outside(&sheet_set(&mut s, sheet, json!({"orientation": "portrait"}))).1, [top, right]);
    let up_to = sheet_set(&mut s, sheet, json!({"size": {"custom": {"sides_mm": [322.5, 297.0]}}, "orientation": "landscape"}));
    assert!(up_to.get("outside").is_none(), "a view on the border is inside: {up_to}");
    assert_eq!(outside(&sheet_set(&mut s, sheet, json!({"size": {"custom": {"sides_mm": [322.4, 297.0]}}}))).1, [top, right]);
    assert!(sheet_set(&mut s, sheet, json!({"name": "Views"})).get("outside").is_none(), "only a change of paper checks");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    let paper = s.drawing("test").unwrap().sheets[0].sheet;
    assert_eq!(paper.size, solidcraft_geom::drawing_sheet::SheetSize::Custom { sides_mm: [322.5, 297.0] });
    let rounding = sheet_set(&mut s, sheet, json!({"size": {"custom": {"sides_mm": [322.5 - solidcraft_geom::EPS / 2.0, 297.0]}}}));
    assert!(rounding.get("outside").is_none(), "within a rounding of the border is inside: {rounding}");
    run(&mut s, "edit.undo", json!({}));
    let added = run(&mut s, "drawing.sheet.add", json!({"size": {"standard": "a4"}, "orientation": "portrait"}));
    let a4 = ids::decode(added["sheet"].as_str().unwrap()).unwrap();
    let moved = to_sheet(&mut s, MOVE, json!({"view": ids::encode(top), "sheet": ids::encode(a4), "position_mm": [150.0, 280.0]}));
    let (listed, views) = outside(&moved);
    assert_eq!(views, [top]);
    assert_box(&listed[0], [137.5, 255.0], [162.5, 305.0]);
    let copied = to_sheet(&mut s, COPY, json!({"view": ids::encode(top), "sheet": ids::encode(a4), "position_mm": [100.0, 100.0]}));
    assert!(copied.get("outside").is_none(), "{copied}");
    // Nothing to measure: the design places more than a view walks.
    for level in 0..=MAX_PLACEMENT_DEPTH as u64 + 1 {
        nest(&mut s, level);
    }
    let (listed, views) = outside(&sheet_set(&mut s, sheet, json!({"size": {"standard": "a4"}})));
    assert_eq!((views, listed[0]["unmeasured"].as_str()), (vec![right], Some("past_walk_limit")));
    assert_box(&listed[0], [290.0, 140.0], [310.0, 160.0]);
}

/// A refusal for a part the view leaves out says where the setting was set, for each kind of setter
/// a resolved setting can name (#4896): a parent names its view, a sheet and a drawing say so, and a
/// view's own setting and the default add nothing.
#[test]
fn a_left_out_part_says_where_each_kind_of_setter_set_it() {
    assert_eq!(set_on(SetBy::Parent(7)), format!(" (set on view {})", ids::encode(7)));
    assert_eq!(set_on(SetBy::Sheet), " (set on its sheet)");
    assert_eq!(set_on(SetBy::Drawing), " (set on the drawing)");
    assert_eq!((set_on(SetBy::This), set_on(SetBy::Default)), (String::new(), String::new()));
}

/// A posed view reads a different placement, so centre references cannot be added there or kept
/// visible when a saved file combines them; an attempted edit is refused without changing the file.
#[test]
fn posed_views_refuse_and_report_centre_annotations() {
    let (mut s, view, _) = hinge_pair();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let (_, other) = create(&mut s, BASE, json!({"sheet": sheet, "view": "top", "scale": "1:1", "position_mm": [250, 150]}));
    let line =
        run(&mut s, "drawing.center_line.add", json!({"view": ids::encode(view), "a": [0, 0, 0], "b": [40, 0, 0], "overshoot_mm": 2.0}))["line"]
            .clone();
    let pose = json!({"pose": [{"joint": "Hinge", "values": [90]}]});
    let before = s.drawing("test").unwrap().clone();
    let error = edit_refused(&mut s, view, pose.clone());
    assert!(error.contains("position of its own") && error.contains("centre annotations"), "{error}");
    assert_eq!(s.drawing("test").unwrap().clone(), before);
    edit_view(&mut s, other, pose);
    for (cmd, params) in [
        ("drawing.center_line.add", json!({"view": ids::encode(other), "a": [0, 0, 0], "b": [40, 0, 0], "overshoot_mm": 2.0})),
        ("drawing.center_mark.add", json!({"view": ids::encode(other), "edge": [0, 0, 0], "overshoot_mm": 2.0})),
    ] {
        let error = s.execute(cmd, &params).unwrap_err().to_string();
        assert!(error.contains("position of its own"), "{error}");
    }
    // Files can contain the combination even though commands refuse it.
    let mut drawing = (**s.drawing("test").unwrap()).clone();
    let saved_pose = drawing.views.iter().find(|v| v.id == other).unwrap().settings.pose.clone();
    let target = drawing.views.iter_mut().find(|v| v.id == view).unwrap();
    Arc::make_mut(target).settings.pose = saved_pose;
    s.doc_mut().put_drawing(Arc::new(drawing));
    let scene = run(&mut s, "drawing.scene", json!({"sheet": sheet}));
    assert!(
        scene["annotation_failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["object"] == line && f["reason"].as_str().unwrap().contains("position of its own")),
        "{scene}"
    );
    let check = run(&mut s, "drawing.check", json!({"sheet": sheet}));
    assert!(
        check["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["kind"] == "failed" && f["object"]["kind"] == "centre_line" && f["object"]["id"] == line),
        "{check}"
    );
    let inspected = run(&mut s, "drawing.center_line.inspect", json!({"line": line}));
    assert!(inspected["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("position of its own")), "{inspected}");
}

/// A view placed apart from its projection place is given the first free letter (#13044), as
/// `projected`, `edit` and `move_to_sheet` place it; a base view and a view lined up with its parent
/// get none, and a view lined up again keeps its own. A copy that doesn't need a letter never takes
/// its original's. With letters not reused, a deleted view's letter never comes back.
#[test]
fn a_view_placed_apart_is_given_a_letter() {
    let mut s = block_drawing();
    let (base, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": "1:2"}));
    let (lined, right) = create(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": "right"}));
    assert!(base.get("letter").is_none() && lined.get("letter").is_none(), "{base} {lined}");
    let above = json!({"parent": ids::encode(front), "side": "above", "place": {"at": [200.0, 250.0]}});
    let (apart, top) = create(&mut s, PROJECTED, above.clone());
    assert_eq!(apart["letter"], "A");
    let letter = |s: &Session, id: u64| saved(s, id).label.map(|label| label.letter);
    let edited = edit_view(&mut s, right, json!({"place": {"at": [350.0, 150.0]}}));
    assert_eq!(edited["changed"], json!(["place", "label"]));
    assert_eq!(letter(&s, right).as_deref(), Some("B"));
    edit_view(&mut s, right, json!({"place": {"aligned": 300.0}}));
    assert_eq!(letter(&s, right).as_deref(), Some("B"), "lined up again, it keeps its letter");
    let second = second_sheet(&mut s);
    let (_, below) = create(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": "below"}));
    to_sheet(&mut s, MOVE, json!({"view": ids::encode(below), "sheet": ids::encode(second), "break_alignment": true}));
    assert_eq!(letter(&s, below).as_deref(), Some("C"), "moved apart");
    // A projected view copied alone is a base view, and the linked views copied with a view are lined up.
    let alone = listed(&to_sheet(&mut s, COPY, json!({"view": ids::encode(top), "sheet": ids::encode(second)})))[0];
    let with = json!({"view": ids::encode(front), "sheet": ids::encode(second), "position_mm": [100.0, 100.0], "with_linked": true});
    let linked = listed(&to_sheet(&mut s, COPY, with))[1];
    assert_eq!((letter(&s, alone), letter(&s, linked)), (None, None), "no letter, never the original's");
    run(&mut s, "drawing.set", json!({"view_letters_reuse": false}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    d.views.retain(|v| v.id != top);
    s.doc_mut().put_drawing(Arc::new(d));
    assert_eq!(create(&mut s, PROJECTED, above).0["letter"], "D", "A isn't given again");
    assert_eq!(run(&mut s, "drawing.inspect", json!({}))["view_letters"], json!({"reuse": false, "given": 4}));
}

/// A label's parts are typed over, hidden and moved by `drawing.view.edit`, one undo step each, and
/// inspect answers them with what the label reads and its box on the sheet: above the view until it
/// is moved, then at its offset from the view's centre. A letter another view holds, a view with no
/// label and hostile parts are refused, changing nothing.
#[test]
fn a_labels_parts_are_typed_over_hidden_and_moved() {
    let mut s = block_drawing();
    let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": "1:2", "position_mm": [150.0, 150.0]}));
    let (_, top) = create(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": "above", "place": {"at": [150.0, 230.0]}}));
    let label = inspect_view(&mut s, top)["label"].clone();
    assert_eq!((label["letter"].clone(), label["text"].clone(), label["needed"].clone()), (json!("A"), json!("VIEW A"), json!(true)));
    let middle = |label: &Value| (point(&label["box_mm"]["min"]) + point(&label["box_mm"]["max"])) * 0.5;
    // The top view is 50 x 25 at 1:2 round (150, 230): its boundary's top is at 242.5.
    assert!(point(&label["box_mm"]["min"]).y > 242.5 && (middle(&label).x - 150.0).abs() < 1.0, "{label}");
    assert!(inspect_view(&mut s, front).get("label").is_none(), "a base view has none");
    let steps = s.undo.len();
    let edited = edit_view(&mut s, top, json!({"label": {"words": "", "name": "BRACKET END", "offset": [0.0, -30.0]}}));
    assert_eq!((edited["changed"].clone(), s.undo.len()), (json!(["label"]), steps + 1));
    let label = inspect_view(&mut s, top)["label"].clone();
    assert_eq!((label["text"].clone(), label["offset"].clone()), (json!("A\nBRACKET END"), json!([0.0, -30.0])));
    assert!(middle(&label).dist(Vec2::new(150.0, 200.0)) < 1.0, "{label}");
    edit_view(&mut s, top, json!({"label": {"offset": null, "shown": false}}));
    let label = inspect_view(&mut s, top)["label"].clone();
    assert_eq!((label["letter"].clone(), label["shown"].clone(), label["text"].clone()), (json!("A"), json!(false), Value::Null));
    assert!(label.get("box_mm").is_none() && label.get("offset").is_none(), "{label}");
    let (_, other) = create(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": "right", "place": {"at": [300.0, 150.0]}}));
    let before = s.drawing("test").unwrap().clone();
    let held = edit_refused(&mut s, other, json!({"label": {"letter": "A"}}));
    assert!(held.contains(&format!("view {} has the letter A", ids::encode(top))), "{held}");
    assert!(edit_refused(&mut s, front, json!({"label": {"name": "X"}})).contains("it has no label"));
    for (bad, why) in [
        (json!({"letter": "a"}), "its letter is one to three capital letters A to Z"),
        (json!({"letter": "ABCD"}), "its letter is one to three capital letters A to Z"),
        (json!({"letter": "A1"}), "its letter is one to three capital letters A to Z"),
        (json!({"words": "two\nlines"}), "a label is one line"),
        (json!({"name": "x".repeat(5000)}), "a label is one line"),
        (json!({"offset": [1e9, 0.0]}), "its label's offset is at most 5000 mm"),
        (json!({"colour": "red"}), "colour"),
    ] {
        let message = edit_refused(&mut s, other, json!({"label": bad}));
        assert!(message.contains(why), "{bad}: {message}");
    }
    assert_eq!(*s.drawing("test").unwrap(), before);
}

/// [`three_views`] in `projection`, with a view projected above the front view and lined up at y 220:
/// the session, and the front view's, the right view's and that view's ids.
fn lined_up_views(projection: Projection) -> (Session, [u64; 3]) {
    let (mut s, [front, right, _]) = three_views();
    set_projection(&mut s, projection);
    let made = run(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": "above", "place": {"aligned": 220.0}}));
    (s, [front, right, ids::decode(made["view"].as_str().unwrap()).unwrap()])
}

fn centre_of(s: &Session, view: u64) -> [f64; 2] {
    let centre = s.drawing("test").unwrap().view_centre(view).unwrap();
    [centre.x, centre.y]
}

fn dragged(s: &mut Session, view: u64, to: [f64; 2]) -> Value {
    let reply = run(s, DRAG, json!({"view": ids::encode(view), "to_mm": to}));
    let schema = output_schema_for::<ViewMoveResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    reply
}

/// Front moved (+20, +5) moves the top view (+20, 0) and the right view (0, +5), in both angles, and
/// a lined-up view dragged off its line takes only its free axis, with its parent still. One step:
/// undo puts all three back. Nothing is evaluated.
#[test]
fn a_dragged_view_keeps_its_line_up_and_its_followers_follow() {
    for projection in [Projection::First, Projection::Third] {
        let (mut s, [front, right, top]) = lined_up_views(projection);
        let evaluations = s.evaluations;
        let reply = dragged(&mut s, front, [170.0, 155.0]);
        assert_eq!(centre_of(&s, front), [170.0, 155.0]);
        assert_eq!(centre_of(&s, right), [300.0, 155.0]);
        assert_eq!(centre_of(&s, top), [170.0, 220.0]);
        assert_eq!(reply["place"], json!({"at": [170.0, 155.0]}), "{reply}");
        let mut followers = [right, top];
        followers.sort_unstable();
        assert_eq!(reply["followers"], json!(followers.map(ids::encode)), "{reply}");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!((centre_of(&s, front), centre_of(&s, right), centre_of(&s, top)), ([150.0, 150.0], [300.0, 150.0], [150.0, 220.0]));

        // Off its line: the right view takes only x.
        let reply = dragged(&mut s, right, [320.0, 999.0]);
        assert_eq!(centre_of(&s, right), [320.0, 150.0]);
        assert_eq!(reply["place"], json!({"aligned": 320.0}), "{reply}");
        assert_eq!(reply["followers"], json!([]), "{reply}");
        // Above: the top view takes only y.
        dragged(&mut s, top, [999.0, 240.0]);
        assert_eq!(centre_of(&s, top), [150.0, 240.0]);
        assert_eq!(s.evaluations, evaluations);
    }
}

/// `break_alignment` places a lined-up view apart, where it was dragged; the views lined up with it
/// go with it, and a view placed at a point takes both axes.
#[test]
fn breaking_alignment_places_a_view_apart() {
    let (mut s, [front, right, _]) = lined_up_views(Projection::Third);
    let reply = run(&mut s, DRAG, json!({"view": ids::encode(right), "to_mm": [320.0, 90.0], "break_alignment": true}));
    assert_eq!(reply["place"], json!({"at": [320.0, 90.0]}), "{reply}");
    assert_eq!(centre_of(&s, right), [320.0, 90.0]);
    assert_eq!(centre_of(&s, front), [150.0, 150.0], "its parent stays");
    let letter = |s: &Session| saved(s, right).label.map(|label| label.letter);
    assert_eq!(letter(&s).as_deref(), Some("A"), "placed apart, it is given a letter");
    dragged(&mut s, right, [330.0, 95.0]);
    assert_eq!(centre_of(&s, right), [330.0, 95.0], "both axes now");
    assert_eq!(letter(&s).as_deref(), Some("A"), "and keeps it");
    // Lined up with a view placed apart, a child follows that view.
    let (mut s, [_, right, top]) = three_views();
    let before = centre_of(&s, top);
    dragged(&mut s, right, [310.0, 0.0]);
    assert_eq!(centre_of(&s, top), before, "placed apart, it stays");
}

/// A move to where it is makes no step, and neither does one that off-axis leaves it on its line. A
/// view off the sheet is listed. Hostile places are refused and change nothing.
#[test]
fn a_move_to_where_it_is_makes_no_step_and_hostile_places_are_refused() {
    let (mut s, [front, right, _]) = lined_up_views(Projection::Third);
    let doc = s.doc.clone();
    dragged(&mut s, front, [150.0, 150.0]);
    assert!(Arc::ptr_eq(&doc, &s.doc), "no step");
    dragged(&mut s, right, [300.0, 777.0]);
    assert!(Arc::ptr_eq(&doc, &s.doc), "still on the same place");

    let out = dragged(&mut s, front, [1000.0, 150.0]);
    assert_eq!(out["outside"][0]["view"], json!(ids::encode(front)), "{out}");
    run(&mut s, "edit.undo", json!({}));

    let doc = s.doc.clone();
    for to in [json!([1e300, 0.0]), json!([0.0, -1e300]), json!([5001.0, 0.0]), json!(["a", 0.0]), json!([0.0]), json!(null)] {
        assert!(s.execute(DRAG, &json!({"view": ids::encode(front), "to_mm": to})).is_err(), "{to}");
    }
    assert!(s.execute(DRAG, &json!({"view": ids::encode(right), "to_mm": [1e300, 0.0]})).is_err());
    assert!(s.execute(DRAG, &json!({"view": "AAAAAAAAAAAA", "to_mm": [0.0, 0.0]})).is_err());
    assert!(Arc::ptr_eq(&doc, &s.doc));
    run(&mut s, "drawing.close", json!({}));
    assert!(s.execute(DRAG, &json!({"view": ids::encode(front), "to_mm": [0.0, 0.0]})).is_err(), "no drawing open");
}

/// A move to the view's own sheet with a position goes by the same rule as a drag: it was ignored.
#[test]
fn a_position_on_the_views_own_sheet_is_a_move() {
    let (mut s, [front, right, _]) = lined_up_views(Projection::Third);
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let reply = run(&mut s, MOVE, json!({"view": ids::encode(front), "sheet": sheet, "position_mm": [170.0, 155.0]}));
    assert_eq!(centre_of(&s, front), [170.0, 155.0]);
    // It, then the views that went with it, as a move to another sheet lists them.
    let views = reply["views"].as_array().cloned().unwrap_or_default();
    assert_eq!((views.first(), views.len()), (Some(&json!(ids::encode(front))), 3), "{reply}");
    let reply = run(&mut s, MOVE, json!({"view": ids::encode(right), "sheet": sheet, "position_mm": [320.0, 999.0]}));
    assert_eq!(centre_of(&s, right), [320.0, 155.0], "on its line");
    assert_eq!(reply["views"], json!([ids::encode(right)]), "{reply}");
    // No position: nothing moves and nothing is listed.
    let reply = run(&mut s, MOVE, json!({"view": ids::encode(front), "sheet": sheet}));
    assert_eq!(reply["views"], json!([]), "{reply}");
}

/// A section view of the front view of [`three_views`], cut along a horizontal line through the
/// block's middle, its arrows down.
fn with_section(s: &mut Session, front: u64) -> u64 {
    use solidcraft_doc::drawing::cutting_line::{ArrowSide, SectionKind};
    section_of(s, front, SectionKind::Full, &[[-40.0, 10.0], [40.0, 10.0]], ArrowSide::Right)
}

/// A section view of `parent`, cut along a line of `kind` through `points` on the parent's plane.
fn section_of(
    s: &mut Session,
    parent: u64,
    kind: solidcraft_doc::drawing::cutting_line::SectionKind,
    points: &[[f64; 2]],
    arrows: solidcraft_doc::drawing::cutting_line::ArrowSide,
) -> u64 {
    use solidcraft_doc::drawing::cutting_line::CuttingLine;
    let mut d = (**s.drawing("test").unwrap()).clone();
    let id = d.allocate_id(&mut solidcraft_doc::ids::IdSource::seeded(13046)).unwrap();
    let line = CuttingLine::new(kind, points.iter().map(|p| Vec2::new(p[0], p[1])).collect(), arrows).unwrap();
    let (sheet, place) = (d.sheets[0].id, ViewPlace::At([150.0, 60.0]));
    let view = View {
        id,
        sheet,
        kind: ViewKind::Section { parent, line },
        place,
        settings: ViewOverrides::default(),
        reference_shown_percent: 0.0,
        centre_marks: None,
        label: None,
    };
    d.add_views([Arc::new(view)]);
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    id
}

/// A section view lists and inspects as a section of its parent, follows its parent's settings and
/// keeps its own editable, copies alone as a section of the same parent, and its sheet draws it with
/// its other views. A dimension on it inspects with no layout and isn't dragged.
#[test]
fn a_section_view_inspects_copies_and_draws() {
    let (mut s, [front, ..]) = three_views();
    let cut = with_section(&mut s, front);
    let reply = inspect_view(&mut s, cut);
    assert_eq!((&reply["kind"], &reply["parent"], reply.get("side")), (&json!("section"), &json!(ids::encode(front)), None), "{reply}");
    assert_eq!((&reply["settings"]["scale"]["from"], &reply["linked"]), (&json!("parent"), &json!(true)), "{reply}");
    // The arrows point down the front view: it looks down, as the top view does.
    assert_eq!(s.drawing("test").unwrap().resolve_view(cut).unwrap().basis.x, NamedView::Top.basis().x);
    let listed_views = run(&mut s, "drawing.inspect", json!({}))["sheets"][0]["views"].clone();
    let entry = listed_views.as_array().unwrap().iter().find(|v| v["id"] == json!(ids::encode(cut))).unwrap();
    assert_eq!(
        (&entry["kind"], &entry["parent"], &entry["label"]),
        (&json!("section"), &json!(ids::encode(front)), &json!("Section of Front")),
        "{entry}"
    );
    edit_view(&mut s, cut, json!({"scale": "1:1"}));
    assert_eq!(saved(&s, cut).settings.scale, Some("1:1".parse().unwrap()));
    // Copied alone, it stays a section of the same parent, its line as it was.
    let copy = listed(&to_sheet(&mut s, COPY, json!({"view": ids::encode(cut), "sheet": "Sheet 1"})))[0];
    assert_eq!(saved(&s, copy).kind, saved(&s, cut).kind);
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let scene = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    let state = |id: u64| scene["views"].as_array().unwrap().iter().find(|v| v["view"] == json!(ids::encode(id))).unwrap().clone();
    assert_eq!((&state(cut)["status"], &state(front)["status"]), (&json!("drawn"), &json!("drawn")), "{scene}");
    // A dimension moved onto it by hand has no layout (dimensions don't read section views yet), and isn't dragged.
    let edge = json!({"point": [50, 0, 0], "path": []});
    let dimension = run(&mut s, "drawing.dimension.add", json!({"view": ids::encode(front), "kind": {"length": {"edge": edge}}}));
    let id = ids::decode(dimension["dimension"].as_str().unwrap()).unwrap();
    s.edit_drawing(EDIT, |d| {
        let at = d.dimensions.iter().position(|r| r.id == id).unwrap();
        Arc::make_mut(&mut d.dimensions[at]).view = cut;
        Ok(())
    })
    .unwrap();
    let read = run(&mut s, "drawing.dimension.inspect", json!({"dimension": dimension["dimension"]}));
    assert_eq!((read.get("layout"), &read["layout_why"]), (None, &json!(crate::drawing_scene::NOT_DRAWN)), "{read}");
    let (record, steps) = (s.drawing("test").unwrap().clone(), s.undo.len());
    let dragged = json!({"dimension": dimension["dimension"], "set": {"text_to_mm": [150, 40]}});
    let e = s.execute("drawing.dimension.edit", &dragged).unwrap_err().to_string();
    assert!(e.contains(crate::drawing_scene::NOT_DRAWN), "{e}");
    assert_eq!((s.drawing("test").unwrap().clone(), s.undo.len()), (record, steps));
}

/// Saved view `id` as its sheet draws it: what it shows, its extents and its lines at its scale,
/// hidden lines too, on paper round its model's middle.
fn drawn_lines(s: &Session, id: u64) -> (ShownSet, Extents, crate::drawing_view::Lines) {
    use crate::drawing_view::{CHORD_MM, LineRequest, PreparedBodies};
    let view = crate::drawing_view::drawn_view(s, s.drawing("test").unwrap(), id).unwrap();
    let shown = crate::drawing_view::shown(s);
    let (set, extents) = crate::drawing_view::measured(&shown, &view).unwrap();
    let frame = crate::drawing_view::frame(view.basis, view.scale.value, &extents.model, Vec2::ZERO, "test").unwrap();
    let request = LineRequest {
        shows: extents.model,
        hidden: true,
        reference_hidden: false,
        reference_hides_model: false,
        tangent_edges: false,
        chord_mm: CHORD_MM,
    };
    let lines = crate::drawing_view::lines(s, &set, &frame, &request, &mut PreparedBodies::default(), "test").unwrap();
    (set.into_owned(), extents, lines)
}

/// Where `lines` has an upright straight piece at `visibility`: its x on paper, in hundredths of a mm.
fn upright(lines: &crate::drawing_view::Lines, visibility: Visibility) -> BTreeSet<i64> {
    let pieces = lines.lines.iter().flat_map(|l| &l.pieces).filter(|p| p.visibility == visibility);
    let segments = pieces.flat_map(|p| &p.path).filter_map(|seg| match seg {
        solidcore_record::Seg2::Line { a, b } if (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() > 1.0 => Some((a.x * 100.0).round() as i64),
        _ => None,
    });
    segments.collect()
}

/// The block drilled upright through its middle (10 mm across) and a tab standing on its left end,
/// [`three_views_of`] them, and a section of the front view along an upright line through the hole,
/// its arrows to the right.
fn drilled_with_tab() -> (Session, [u64; 3], u64) {
    use solidcraft_doc::drawing::cutting_line::{ArrowSide, SectionKind};
    let mut s = block();
    run(&mut s, "solid.cylinder", json!({"diameter": 10, "height": 22, "base": [50, 25, -1], "operation": "cut", "targets": ["Block"]}));
    run(&mut s, "solid.box", json!({"corner": [0, 0, 20], "length": 20, "width": 50, "height": 40, "body_name": "Tab"}));
    let (mut s, views) = three_views_of(s);
    let cut = section_of(&mut s, views[0], SectionKind::Full, &[[50.0, -10.0], [50.0, 70.0]], ArrowSide::Right);
    (s, views, cut)
}

/// A full section view draws its parent's bodies cut along its line, the side its arrows point to
/// kept: the tab on the other side is left out, so the view is the block's 50 x 20 mm end, and the
/// hole's two sides, 5 mm from its middle, are cut edges, drawn visible. Seen whole from the right,
/// they are hidden. Asked again, the view takes the cut it made.
#[test]
fn a_full_section_view_draws_the_bodies_cut_along_its_line() {
    let (s, [_, right, _], cut) = drilled_with_tab();
    let (set, extents, lines) = drawn_lines(&s, cut);
    let names: Vec<&str> = set.bodies.iter().map(|b| b.body.as_str()).collect();
    assert_eq!((names, &set.uncut), (vec!["Block"], &None));
    let size = extents.model.max - extents.model.min;
    assert!((size.x - 50.0).abs() < 1e-6 && (size.y - 20.0).abs() < 1e-6, "{size:?}");
    // At 1:2, 5 mm from the middle is 2.5 mm on paper.
    let sides = BTreeSet::from([-250, 250]);
    assert!(sides.is_subset(&upright(&lines, Visibility::Visible)), "{:?}", upright(&lines, Visibility::Visible));
    let (whole, _, seen) = drawn_lines(&s, right);
    assert_eq!(whole.bodies.len(), 2);
    assert!(sides.is_disjoint(&upright(&seen, Visibility::Visible)), "{:?}", upright(&seen, Visibility::Visible));
    assert!(sides.is_subset(&upright(&seen, Visibility::Hidden)), "{:?}", upright(&seen, Visibility::Hidden));
    let (again, ..) = drawn_lines(&s, cut);
    let body = |set: &ShownSet| set.body(&set.bodies[0]).unwrap().body.clone();
    assert!(body(&again).same(&body(&set)));
}

/// A section's cut bodies never lend their geometry to another copy of the part, as the plane cuts
/// each copy where it stands: with the line through the first of two cubes, each copy is a group of
/// its own, while the parent's view still shares one copy's geometry between them.
#[test]
fn copies_of_a_part_are_cut_apart() {
    use solidcraft_doc::drawing::cutting_line::{ArrowSide, SectionKind};
    let (s, _) = two_cubes();
    let (mut s, [front, ..]) = three_views_of(s);
    let cut = section_of(&mut s, front, SectionKind::Full, &[[5.0, -10.0], [5.0, 20.0]], ArrowSide::Right);
    let groups = |id: u64| {
        let view = crate::drawing_view::drawn_view(&s, s.drawing("test").unwrap(), id).unwrap();
        let shown = crate::drawing_view::shown(&s);
        let (set, _) = crate::drawing_view::measured(&shown, &view).unwrap();
        let placed = crate::mesh_lines::placed(&s, &set.state, |_| true, "test").unwrap();
        placed.iter().map(|p| p.group).collect::<BTreeSet<_>>().len()
    };
    assert_eq!((groups(front), groups(cut)), (1, 2));
}

/// Reference parts show whole around a section, as context, while the model parts are cut: the
/// plate is cut where the line crosses it, and the block over it, a reference part, isn't.
#[test]
fn reference_parts_are_not_cut() {
    use solidcraft_doc::drawing::cutting_line::{ArrowSide, SectionKind};
    let (s, _) = plate_and_block(5.0);
    let (mut s, [front, ..]) = three_views_of(s);
    let cut = section_of(&mut s, front, SectionKind::Full, &[[35.0, -10.0], [35.0, 30.0]], ArrowSide::Right);
    let world = crate::drawing_view::shown(&s).unwrap();
    let (set, ..) = drawn_lines(&s, cut);
    let whole = |reference: bool| {
        let shown = set.bodies.iter().find(|b| b.reference == reference).unwrap();
        set.body(shown).unwrap().body.same(&world.body(shown).unwrap().body)
    };
    assert_eq!((set.bodies.len(), whole(true), whole(false)), (2, true, false));
}

/// A mesh body has no faces to cut: a section draws it whole beside the cut block, and says so.
#[test]
fn a_mesh_body_is_drawn_uncut_and_said_so() {
    use solidcraft_doc::drawing::cutting_line::{ArrowSide, SectionKind};
    let mut s = block();
    with_mesh_cube(&mut s, [60.0, 10.0, 30.0], "section");
    let (mut s, [front, ..]) = three_views_of(s);
    let cut = section_of(&mut s, front, SectionKind::Full, &[[50.0, -10.0], [50.0, 70.0]], ArrowSide::Right);
    let (set, ..) = drawn_lines(&s, cut);
    assert_eq!((set.bodies.len(), set.uncut.as_deref()), (2, Some("1 mesh body is drawn uncut: a mesh has no faces to cut")));
}

/// A line that keeps none of the parts gives the placeholder, and says why.
#[test]
fn a_line_that_keeps_none_of_the_parts_shows_the_placeholder() {
    use solidcraft_doc::drawing::cutting_line::{ArrowSide, SectionKind};
    let (mut s, [front, ..]) = three_views();
    let cut = section_of(&mut s, front, SectionKind::Full, &[[-40.0, 0.0], [40.0, 0.0]], ArrowSide::Right);
    let view = crate::drawing_view::drawn_view(&s, s.drawing("test").unwrap(), cut).unwrap();
    assert_eq!(crate::drawing_view::measured(&crate::drawing_view::shown(&s), &view).err(), Some(Unmeasured::CutAway));
    let reply = inspect_view(&mut s, cut);
    assert_eq!(reply["extents"]["provenance"], json!("cut_away"), "{reply}");
}

/// Only a full section's lines are drawn yet: a half section, and a shaded full section, say so,
/// and their sheet draws its other views.
#[test]
fn other_sections_and_shaded_ones_wait() {
    use solidcraft_doc::drawing::cutting_line::{ArrowSide, SectionKind};
    let (mut s, [front, ..]) = three_views();
    let half = section_of(&mut s, front, SectionKind::Half, &[[-40.0, 10.0], [50.0, 10.0], [50.0, 30.0]], ArrowSide::Right);
    let full = with_section(&mut s, front);
    edit_view(&mut s, full, json!({"visual_style": "shaded"}));
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let scene = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    let state = |id: u64| scene["views"].as_array().unwrap().iter().find(|v| v["view"] == json!(ids::encode(id))).unwrap().clone();
    assert_eq!(state(half)["reason"], json!("not supported yet: a half section"), "{scene}");
    assert_eq!(state(full)["reason"], json!("not supported yet: a shaded section view"), "{scene}");
    assert_eq!(state(front)["status"], json!("drawn"), "{scene}");
}

/// A pictorial view's boundary is the gap past its parent's on both axes, corner to corner: above
/// right of the 50 x 10 front view at 1:2 (185..235 x 143.5..153.5), the block's isometric view,
/// 150 / sqrt 2 / 2 = 53.03 wide and 190 / sqrt 6 / 2 = 38.78 high, goes to 255..308.03 x
/// 173.5..212.28, saved at that box's centre, at its parent's scale, looking from the corner
/// between the top and right views (`iso_front_right`) and following its parent. Below left it
/// looks from below in third angle, and is the same named view in first. A point given is kept; a
/// place lined up is refused.
#[test]
fn a_pictorial_view_sits_a_gap_past_its_parents_corner() {
    let mut s = block_drawing();
    let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": "1:2"}));
    let (wide, high) = (150.0 / 2f64.sqrt() / 2.0, 190.0 / 6f64.sqrt() / 2.0);
    let (iso, iso_id) = create(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": "above_right"}));
    assert_box(&iso, [255.0, 173.5], [255.0 + wide, 173.5 + high]);
    assert_eq!(iso["scale"]["text"], "1:2");
    let view = saved(&s, iso_id);
    assert_eq!(view.kind, ViewKind::Pictorial { parent: front, diagonal: Diagonal::AboveRight });
    let ViewPlace::At([x, y]) = view.place else { panic!("placed at a point: {:?}", view.place) };
    assert!((Vec2::new(x, y) - Vec2::new(255.0 + wide / 2.0, 173.5 + high / 2.0)).len() < EPS, "{x} {y}");
    let inspected = inspect_view(&mut s, iso_id);
    assert_eq!(
        (&inspected["kind"], &inspected["side"], &inspected["parent"]),
        (&json!("pictorial"), &json!("above_right"), &json!(ids::encode(front)))
    );
    assert_eq!(inspected["direction"]["named"], "iso_front_right");
    assert_eq!((&inspected["settings"]["scale"]["from"], &inspected["linked"]), (&json!("parent"), &json!(true)));

    let (below, below_id) = create(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": "below_left"}));
    assert_box(&below, [185.0 - 20.0 - wide, 143.5 - 20.0 - high], [165.0, 123.5]);
    assert_eq!(inspect_view(&mut s, below_id)["direction"].get("named"), None, "third angle: from below");
    set_projection(&mut s, Projection::First);
    assert_eq!(inspect_view(&mut s, below_id)["direction"]["named"], "iso_front_right", "first angle");
    set_projection(&mut s, Projection::Third);

    let (_, apart) = create(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": "above_left", "place": {"at": [300.0, 60.0]}}));
    assert_eq!(saved(&s, apart).place, ViewPlace::At([300.0, 60.0]));
    let lined = create_refused(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": "above_right", "place": {"aligned": 300.0}}));
    assert!(lined.contains("a pictorial view is placed at a point"), "{lined}");
}

/// An isometric view of a 100 x 60 x 20 block at 1:1 is its outline at the isometric ratio:
/// 160 / sqrt 2 = 113.14 wide and 200 / sqrt 6 = 81.65 high.
#[test]
fn an_isometric_view_of_a_100_60_20_block_is_its_isometric_outline() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 20, "body_name": "Block"}));
    run(&mut s, "drawing.create", json!({}));
    let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": "1:1", "position_mm": [100.0, 100.0]}));
    let (iso, _) = create(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": "above_right"}));
    let (size, _) = boundary(&iso);
    assert!((size.x - 160.0 / 2f64.sqrt()).abs() < EPS && (size.y - 200.0 / 6f64.sqrt()).abs() < EPS, "{size:?}");
}

/// `drawing.view.projection_for` names the view a point gives, under the drawing's projection: in
/// third angle 80 mm above the front view's centre the top view, 80 mm right the right view; in
/// first angle 80 mm below gives the top view and 80 mm left the right view; on a diagonal an
/// isometric view, not lined up, at the parent's scale. On the centre, or from a pictorial parent,
/// none, with why. It reads no model, saves nothing, and agrees with what `drawing.view.projected`
/// makes for each of the eight.
#[test]
fn the_view_beside_a_point_follows_the_drawings_projection() {
    let mut s = block_drawing();
    let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": "1:2"}));
    let ask = |s: &mut Session, offset: [f64; 2]| {
        let reply = run(s, PROJECTION_FOR, json!({"parent": ids::encode(front), "offset_mm": offset}));
        let schema = output_schema_for::<ProjectionForResult>();
        assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
        reply
    };
    let (steps, evaluations, record) = (s.undo.len(), s.evaluations, s.drawing("test").unwrap().clone());
    let above = ask(&mut s, [0.0, 80.0]);
    assert_eq!(
        (&above["kind"], &above["side"], &above["lined_up"], &above["direction"]["named"], &above["scale"]["text"], &above["projection"]),
        (&json!("orthographic"), &json!("above"), &json!(true), &json!("top"), &json!("1:2"), &json!("third"))
    );
    let right = ask(&mut s, [80.0, 0.0]);
    assert_eq!((&right["side"], &right["direction"]["named"]), (&json!("right"), &json!("right")));
    let iso = ask(&mut s, [80.0, 80.0]);
    assert_eq!(
        (&iso["kind"], &iso["side"], &iso["lined_up"], &iso["direction"]["named"], &iso["scale"]["text"]),
        (&json!("pictorial"), &json!("above_right"), &json!(false), &json!("iso_front_right"), &json!("1:2"))
    );
    let centre = ask(&mut s, [0.0, 0.0]);
    assert_eq!((&centre["kind"], centre.get("side"), &centre["why"]), (&json!("none"), None, &json!("on the parent's centre")));
    assert!(ask(&mut s, [10000.0, -10000.0])["kind"] == "pictorial");
    assert_eq!((s.undo.len(), s.evaluations, s.drawing("test").unwrap().clone()), (steps, evaluations, record), "reads no model, saves nothing");

    set_projection(&mut s, Projection::First);
    let below = ask(&mut s, [0.0, -80.0]);
    assert_eq!((&below["side"], &below["direction"]["named"], &below["projection"]), (&json!("below"), &json!("top"), &json!("first")));
    assert_eq!(ask(&mut s, [-80.0, 0.0])["direction"]["named"], "right");
    assert_eq!(ask(&mut s, [-80.0, -80.0])["direction"]["named"], "iso_front_right");
    set_projection(&mut s, Projection::Third);

    for (offset, side) in [
        ([80.0, 0.0], "right"),
        ([-80.0, 0.0], "left"),
        ([0.0, 80.0], "above"),
        ([0.0, -80.0], "below"),
        ([80.0, 80.0], "above_right"),
        ([-80.0, 80.0], "above_left"),
        ([80.0, -80.0], "below_right"),
        ([-80.0, -80.0], "below_left"),
    ] {
        let answer = ask(&mut s, offset);
        assert_eq!(answer["side"], side);
        let (_, made) = create(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": side, "place": {"at": [300.0, 60.0]}}));
        assert_eq!(inspect_view(&mut s, made)["direction"], answer["direction"], "{side}");
        if side == "above_right" {
            let from_iso = run(&mut s, PROJECTION_FOR, json!({"parent": ids::encode(made), "offset_mm": [80.0, 0.0]}));
            assert_eq!(from_iso["kind"], "none");
            assert_eq!(from_iso["why"], "not supported yet: a view projected from a pictorial view");
        }
    }
}

const FLAT: &str = "drawing.view.flat";

/// `drawing.view.flat` with `p`: the new view's id and the reply, checked against its schema.
fn flat_view(s: &mut Session, p: Value) -> (u64, Value) {
    let reply = run(s, FLAT, p);
    let schema = output_schema_for::<CreateResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    (ids::decode(reply["view"].as_str().unwrap()).unwrap(), reply)
}

/// `sheet.flat_pattern`'s reply, read with the drawing closed while one is open, as these tests
/// have always read it.
fn flat_pattern(s: &mut Session) -> Value {
    if s.open_drawing.is_none() {
        return run(s, "sheet.flat_pattern", json!({}));
    }
    let mut reply = Value::Null;
    with_the_model(s, |s| reply = run(s, "sheet.flat_pattern", json!({})));
    reply
}

/// The flat pattern's size, width and height.
fn flat_size(s: &mut Session) -> Vec2 {
    point(&flat_pattern(s)["flat_size_mm"])
}

/// View `id`'s strokes on its sheet, drawn at once: each path with its dash lengths, and the view's
/// state.
fn flat_strokes(s: &Session, id: u64) -> (Vec<(Vec<solidcore_record::Seg2>, Option<Vec<f64>>)>, crate::drawing_scene::Status) {
    let d = s.drawing("test").unwrap();
    let sheet = d.view(id).unwrap().sheet;
    let built = crate::drawing_scene::build(s, d, sheet).unwrap();
    let status = built.views.iter().find(|v| v.view == id).unwrap().status.clone();
    let items = built.scene.groups.iter().filter(|g| g.object == id).flat_map(|g| &g.items);
    let stroke = |item: &solidcraft_geom::paper::Item| match item {
        solidcraft_geom::paper::Item::Stroke { path, pattern, .. } => Some((path.clone(), pattern.as_ref().map(|p| p.lengths_mm.clone()))),
        _ => None,
    };
    (items.filter_map(stroke).collect(), status)
}

/// The box round the ends of `segs`' lines.
fn line_box<'a>(segs: impl Iterator<Item = &'a solidcore_record::Seg2>) -> Aabb2 {
    segs.filter_map(|seg| match seg {
        solidcore_record::Seg2::Line { a, b } => Some([*a, *b]),
        _ => None,
    })
    .flatten()
    .collect()
}

/// How far `p` is from the nearest line of `segs`.
fn off_lines<'a>(p: Vec2, segs: impl Iterator<Item = &'a solidcore_record::Seg2>) -> f64 {
    let from = |a: Vec2, b: Vec2| {
        let t = ((p - a).dot(b - a) / (b - a).dot(b - a)).clamp(0.0, 1.0);
        (a + (b - a) * t - p).len()
    };
    segs.filter_map(|seg| if let solidcore_record::Seg2::Line { a, b } = seg { Some(from(*a, *b)) } else { None }).fold(f64::INFINITY, f64::min)
}

/// The issue's plate: a flat view's boundary, and its outline on the sheet, measure the flat size
/// `sheet.flat_pattern` reports, to 1e-6 mm, at 1:1 and at 1:2, centred where it was placed.
#[test]
fn a_flat_view_measures_the_flat_size_at_its_scale() {
    let mut s = sheet_plate();
    let size = flat_size(&mut s);
    for (scale, factor) in [("1:1", 1.0), ("1:2", 0.5)] {
        let (id, reply) = flat_view(&mut s, json!({"sheet": "Sheet 1", "body": "Plate", "scale": scale, "position_mm": [200, 150]}));
        let (on_sheet, centre) = boundary(&reply);
        assert!((on_sheet - size * factor).len() < 1e-6 && (centre - Vec2::new(200.0, 150.0)).len() < 1e-6, "{scale}: {reply}");
        let (strokes, status) = flat_strokes(&s, id);
        assert_eq!(status.name(), "drawn", "{status:?}");
        let outline = line_box(strokes.iter().filter(|(_, dashes)| dashes.is_none()).flat_map(|(path, _)| path));
        assert!((outline.size() - size * factor).len() < 1e-6 && (outline.center() - Vec2::new(200.0, 150.0)).len() < 1e-6, "{scale}: {outline:?}");
    }
}

/// A round hole is one circle of its exact diameter on the flat view, never a polyline, and each
/// loop of the flat is drawn once.
#[test]
fn a_round_hole_is_one_circle_on_a_flat_view() {
    let mut s = sheet_plate();
    with_the_model(&mut s, |s| {
        run(s, "sketch.create", json!({"plane": "XY", "name": "Hole"}));
        run(s, "sketch.circle.center", json!({"center": [50, 30], "radius": 5}));
        run(s, "sketch.finish", json!({}));
        run(s, "solid.extrude", json!({"sketch": "Hole", "through_all": true, "direction": "symmetric", "operation": "cut"}));
    });
    let flat = flat_pattern(&mut s);
    let loops = flat["outline"].as_array().unwrap().len() + flat["cutouts"].as_array().unwrap().len();
    let (id, _) = flat_view(&mut s, json!({"sheet": "Sheet 1", "scale": "1:2"}));
    let (strokes, _) = flat_strokes(&s, id);
    let solid: Vec<_> = strokes.iter().filter(|(_, dashes)| dashes.is_none()).map(|(path, _)| path).collect();
    assert_eq!(solid.len(), loops, "one stroke per loop");
    let circle = |path: &&Vec<solidcore_record::Seg2>| {
        let arcs: Option<Vec<(Vec2, f64, f64)>> = path
            .iter()
            .map(|seg| if let solidcore_record::Seg2::Arc { center, radius, sweep, .. } = seg { Some((*center, *radius, *sweep)) } else { None })
            .collect();
        arcs.filter(|arcs| arcs.iter().all(|(c, r, _)| (*r - 2.5).abs() < 1e-9 && (*c - arcs[0].0).len() < 1e-9))
            .is_some_and(|arcs| (arcs.iter().map(|(_, _, sweep)| sweep.abs()).sum::<f64>() - std::f64::consts::TAU).abs() < 1e-9)
    };
    assert_eq!(solid.iter().copied().filter(circle).count(), 1, "{solid:?}");
}

/// A flat pattern view draws one body's flat, so it is never exploded or posed, it doesn't follow the
/// browser's eyes (#22107), and it doesn't draw a configuration of its own yet. Edit refuses each and
/// changes nothing. In a drawing saved so by hand, the edit rule refuses it too, and the sheet draws no
/// lines for the view and says why.
#[test]
fn a_flat_pattern_view_is_never_exploded_posed_or_configured() {
    let mut s = sheet_plate();
    with_the_model(&mut s, |s| {
        run(s, "parameters.add", json!({"name": "W", "expression": "60 mm"}));
        run(s, "config.configure", json!({"params": ["W"]}));
        run(s, "config.row", json!({"name": "Wide", "values": {"W": "80 mm"}}));
    });
    let (id, _) = flat_view(&mut s, json!({"sheet": "Sheet 1", "scale": "1:1"}));
    let configured = "not supported yet: a flat pattern view of a configuration of its own";
    let (record, steps) = (s.drawing("test").unwrap().clone(), s.undo.len());
    let e = edit_refused(&mut s, id, json!({"configuration": "Wide"}));
    assert!(e.contains(configured), "{e}");
    assert_eq!((s.drawing("test").unwrap().clone(), s.undo.len()), (record.clone(), steps));
    let e = edit_refused(&mut s, id, json!({"shows": "design"}));
    assert!(e.contains("whatever the browser shows"), "{e}");
    assert_eq!((s.drawing("test").unwrap().clone(), s.undo.len()), (record.clone(), steps));
    let cases: [(fn(&mut solidcraft_doc::drawing_view::ViewOverrides), &str); 4] = [
        (|o| o.exploded = Some(Exploded::View(1)), "a flat pattern view is never exploded: it draws one body's flat"),
        (|o| o.pose = Some(Pose::Study(StudyStep { id: 1, step: 0 })), "a flat pattern view has no position: it draws one body's flat"),
        (|o| o.configuration = Some(ViewConfiguration::Row(1)), configured),
        (
            |o| o.shows = Some(solidcraft_doc::drawing_view::Shows::Design),
            "a flat pattern view draws its body's flat whatever the browser shows: it can't show only what the design shows",
        ),
    ];
    for (set, why) in cases {
        s.edit_drawing(EDIT, |d| {
            let at = d.views.iter().position(|v| v.id == id).unwrap();
            set(&mut Arc::make_mut(&mut d.views[at]).settings);
            Ok(())
        })
        .unwrap();
        let e = check_apart(s.drawing(EDIT).unwrap(), std::iter::once(id), EDIT).unwrap_err().to_string();
        assert!(e.contains(why), "{e}");
        let (strokes, status) = flat_strokes(&s, id);
        assert_eq!((strokes.len(), status), (0, crate::drawing_scene::Status::Failed(why.into())));
        s.edit_drawing(EDIT, |d| {
            *d = Drawing::clone(&record);
            Ok(())
        })
        .unwrap();
    }
}

/// A flat pattern view draws its body's flat, not the model's edges, so nothing on it points at the
/// model. A dimension, dimensions from the model, a balloon, a note on an edge, a note's leader and a
/// leader added to a note are each refused, and nothing changes. A note on the view that points at
/// nothing is drawn. Saved on it by hand, a dimension, a note on an edge and a balloon are left out of
/// the sheet, which says they have no place to draw; the dimension inspects with that reason and
/// isn't dragged.
#[test]
fn nothing_on_a_flat_pattern_view_points_at_the_model() {
    let mut s = sheet_plate();
    let (flat, _) = flat_view(&mut s, json!({"sheet": "Sheet 1", "scale": "1:1", "position_mm": [100, 150]}));
    let (_, base) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "view": "top", "scale": "1:1", "position_mm": [300, 150]}));
    let (on_flat, on_base) = (ids::encode(flat), ids::encode(base));
    let why = "a flat pattern view draws its body's flat, not the model's edges: nothing on it points at the model";
    let edge = json!({"point": [50, 60, 2], "path": []});
    let plain = run(&mut s, "drawing.note.add", json!({"anchor": {"view": on_flat}, "at": [0, -50], "content": "FLAT"}));
    let plain = ids::decode(plain["note"].as_str().unwrap()).unwrap();
    let asks = [
        ("drawing.dimension.add", json!({"view": on_flat, "kind": {"length": {"edge": edge}}})),
        ("drawing.dimension.from_model", json!({"view": on_flat})),
        ("drawing.balloon.add", json!({"view": on_flat, "edge": edge, "at": [40, 40]})),
        ("drawing.note.add", json!({"anchor": {"edge": {"view": on_flat, "edge": edge}}, "at": [40, 40], "content": "Edge"})),
        ("drawing.note.add", json!({"anchor": {"view": on_flat}, "at": [40, 40], "content": "Edge", "branches": [{"edge": edge}]})),
        ("drawing.note.edit", json!({"note": ids::encode(plain), "set": {"branches": {"add": [{"edge": edge}]}}})),
    ];
    for (cmd, p) in asks {
        let (record, steps) = (s.drawing("test").unwrap().clone(), s.undo.len());
        let e = s.execute(cmd, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {cmd} {p}"));
        assert!(e.contains(why), "{cmd}: {e}");
        assert_eq!((s.drawing("test").unwrap().clone(), s.undo.len()), (record, steps), "{cmd}");
    }
    // Made on the base view, then moved onto the flat view by hand.
    let dimension = run(&mut s, "drawing.dimension.add", json!({"view": on_base, "kind": {"length": {"edge": edge}}}));
    let dimension = ids::decode(dimension["dimension"].as_str().unwrap()).unwrap();
    let noted = run(&mut s, "drawing.note.add", json!({"anchor": {"edge": {"view": on_base, "edge": edge}}, "at": [40, 40], "content": "Edge"}));
    let noted = ids::decode(noted["note"].as_str().unwrap()).unwrap();
    s.edit_drawing(EDIT, |d| {
        let at = d.dimensions.iter().position(|r| r.id == dimension).unwrap();
        Arc::make_mut(&mut d.dimensions[at]).view = flat;
        let at = d.notes.index_of(noted).unwrap();
        if let Some(solidcraft_doc::drawing::note::Anchor::Edge { view, .. }) = d.notes.get_mut(at).map(|note| &mut Arc::make_mut(note).anchor) {
            *view = flat;
        }
        // No command puts a balloon there, and this design has no placed part: one saved by hand.
        let id = d.allocate_id(&mut solidcraft_doc::ids::IdSource::seeded(13030)).unwrap();
        let edge = solidcraft_doc::drawing::dimension::ModelEdge { body: "Plate".into(), origin_feature: 1, name: "F1:side".into(), path: vec![7] };
        d.balloons.add([Arc::new(solidcraft_doc::drawing::balloon::Balloon {
            id,
            view: flat,
            edge,
            at: [40.0, 40.0],
            parts_list: None,
            along: None,
        })]);
        Ok(())
    })
    .unwrap();
    // Inspect and a drag place it through the view's one rule (`on_its_sheet`): no place there either.
    let on_flat = json!({"dimension": ids::encode(dimension)});
    let read = run(&mut s, "drawing.dimension.inspect", on_flat.clone());
    assert_eq!((read.get("layout"), &read["layout_why"]), (None, &json!(crate::drawing_scene::NO_PLACE)), "{read}");
    let (record, steps) = (s.drawing("test").unwrap().clone(), s.undo.len());
    let dragged = json!({"dimension": on_flat["dimension"], "set": {"text_to_mm": [120, 160]}});
    let e = s.execute("drawing.dimension.edit", &dragged).unwrap_err().to_string();
    assert!(e.contains(crate::drawing_scene::NO_PLACE), "{e}");
    assert_eq!((s.drawing("test").unwrap().clone(), s.undo.len()), (record, steps));
    let d = s.drawing("test").unwrap();
    let built = crate::drawing_scene::build(&s, d, d.sheets[0].id).unwrap();
    let dimension = built.dimensions.iter().find(|one| one.dimension == dimension).unwrap();
    assert_eq!((dimension.view, dimension.drawn, dimension.why.as_deref()), (flat, false, Some(crate::drawing_scene::NO_PLACE)));
    let note = |id: u64| built.notes.iter().find(|one| one.note == id).unwrap();
    assert_eq!((note(noted).view, note(noted).drawn, note(noted).why.as_deref()), (Some(flat), false, Some(crate::drawing_scene::NO_PLACE)));
    assert_eq!((note(plain).drawn, note(plain).why.as_deref()), (true, None));
    let [balloon] = built.balloons.as_slice() else { panic!("one balloon: {:?}", built.balloons) };
    assert_eq!((balloon.view, balloon.drawn, balloon.why.as_deref()), (flat, false, Some(crate::drawing_scene::NO_PLACE)));
}

/// A bend up is ISO 128's long dash and dot (type 04), a bend down its long dash and two dots (type
/// 05), with ISO 128-20 Table 3's lengths: long dashes 24 widths of the narrow line, gaps 3, dots 0.5,
/// in every line group. In the 0.35 mm group a bend up is 4.32, 0.54, 0.09, 0.54 mm. Each, narrow, is a
/// stroke the scene takes across an A0 sheet.
#[test]
fn bend_lines_are_long_dashes_with_one_and_two_dots_in_every_group() {
    use solidcraft_geom::paper::{Dashes, Group, Item, ObjectKind, Scene};
    let groups = [LineGroup::Mm0_25, LineGroup::Mm0_35, LineGroup::Mm0_5, LineGroup::Mm0_7, LineGroup::Mm1, LineGroup::Mm1_4, LineGroup::Mm2];
    let path = vec![solidcore_record::Seg2::Line { a: Vec2::new(10.0, 10.0), b: Vec2::new(1180.0, 830.0) }];
    for group in groups {
        let d = LineKind::Thin.width_mm(group);
        assert_eq!(bend_dashes_mm(true, group), vec![24.0 * d, 3.0 * d, 0.5 * d, 3.0 * d], "{group:?}");
        assert_eq!(bend_dashes_mm(false, group), vec![24.0 * d, 3.0 * d, 0.5 * d, 3.0 * d, 0.5 * d, 3.0 * d], "{group:?}");
        let items = [true, false].map(|up| Item::Stroke {
            path: path.clone(),
            closed: false,
            width_mm: d,
            pattern: Some(Dashes { lengths_mm: bend_dashes_mm(up, group), phase_mm: 0.0 }),
        });
        let scene = Scene::new(Vec2::new(1189.0, 841.0), vec![Group { object: 1, kind: ObjectKind::View, items: items.to_vec() }]);
        scene.validate().unwrap_or_else(|e| panic!("{group:?}: {e}"));
    }
    for (got, want) in bend_dashes_mm(true, LineGroup::Mm0_35).iter().zip([4.32, 0.54, 0.09, 0.54]) {
        assert!((got - want).abs() < 1e-12, "{got} {want}");
    }
}

/// Each bend line's chain-line pattern starts so its long dash is centred on the line: at half the
/// line's length the pattern is halfway along its first, long dash.
#[test]
fn a_bend_line_has_its_long_dash_at_its_middle() {
    let mut s = sheet_plate();
    with_the_model(&mut s, |s| {
        run(s, "sheet.flange", json!({"edges": [[50, 60, 2]], "height": 20, "flip": true}));
    });
    let (id, _) = flat_view(&mut s, json!({"sheet": "Sheet 1", "scale": "1:1"}));
    let d = s.drawing("test").unwrap();
    let built = crate::drawing_scene::build(&s, d, d.view(id).unwrap().sheet).unwrap();
    let mut bends = 0;
    for item in built.scene.groups.iter().filter(|g| g.object == id).flat_map(|g| &g.items) {
        let solidcraft_geom::paper::Item::Stroke { path, pattern: Some(dashes), .. } = item else { continue };
        let period: f64 = dashes.lengths_mm.iter().sum();
        let length: f64 = path.iter().map(solidcore_record::Seg2::length).sum();
        let off = ((dashes.phase_mm + length / 2.0).rem_euclid(period) - dashes.lengths_mm[0] / 2.0).abs();
        assert!(off.min(period - off) < 1e-9, "a {length} mm bend line: {dashes:?}");
        bends += 1;
    }
    assert_eq!(bends, 2, "one bend line up, one down");
}

/// A bend up and a bend down are drawn in two line types, ISO 128's long dash and dot and its long
/// dash and two dots, narrow, one stroke per bend line, each where its own bend is; each bend line
/// ends on the outline, unturned and turned a quarter.
#[test]
fn bends_up_and_down_draw_in_two_line_types_and_end_on_the_outline() {
    let mut s = sheet_plate();
    with_the_model(&mut s, |s| {
        run(s, "sheet.flange", json!({"edges": [[50, 60, 2]], "height": 20, "flip": true}));
    });
    let reply = flat_pattern(&mut s);
    let bends = reply["bends"].as_array().unwrap();
    let count = |direction: &str| bends.iter().filter(|b| b["direction"] == direction).count();
    assert_eq!((count("up"), count("down")), (1, 1), "{bends:?}");
    let (id, view) = flat_view(&mut s, json!({"sheet": "Sheet 1", "scale": "1:1"}));
    let (strokes, _) = flat_strokes(&s, id);
    let group = LineGroup::DEFAULT;
    let (up, down) = (Some(bend_dashes_mm(true, group)), Some(bend_dashes_mm(false, group)));
    let drawn = |dashes: &Option<Vec<f64>>| strokes.iter().filter(|(_, d)| d == dashes).map(|(path, _)| path).collect::<Vec<_>>();
    assert_eq!((drawn(&up).len(), drawn(&down).len()), (1, 1), "{strokes:?}");
    // Unturned at 1:1, the flat only moves onto the sheet: its box's corner goes to the boundary's.
    let flat = crate::drawing_view::flat_from(&reply, QuarterTurn::None, FLAT).unwrap().model;
    let on_sheet = |p: &Value| point(p) - flat.min + point(&view["boundary_mm"]["min"]);
    for bend in bends {
        let dashes = if bend["direction"] == "up" { &up } else { &down };
        let [solidcore_record::Seg2::Line { a, b }] = drawn(dashes)[0].as_slice() else { panic!("a bend line is one line") };
        let (start, end) = (on_sheet(&bend["start"]), on_sheet(&bend["end"]));
        let at = |p: Vec2, q: Vec2| (*a - p).len() < 1e-9 && (*b - q).len() < 1e-9;
        assert!(at(start, end) || at(end, start), "{bend}: drawn from {a:?} to {b:?}");
    }
    // Turned a quarter too, the bend lines turn with the outline and end on it.
    let (turned, _) = flat_view(&mut s, json!({"sheet": "Sheet 1", "scale": "1:1", "turn": 90}));
    for strokes in [strokes, flat_strokes(&s, turned).0] {
        let outline: Vec<_> = strokes.iter().filter(|(_, d)| d.is_none()).flat_map(|(path, _)| path).collect();
        let bend_lines: Vec<_> = strokes.iter().filter(|(_, d)| *d == up || *d == down).map(|(path, _)| path).collect();
        assert_eq!(bend_lines.len(), 2, "{strokes:?}");
        for path in bend_lines {
            let [solidcore_record::Seg2::Line { a, b }] = path.as_slice() else { panic!("a bend line is one line: {path:?}") };
            for end in [*a, *b] {
                assert!(off_lines(end, outline.iter().copied()) < 1e-6, "{end:?} isn't on the outline");
            }
        }
    }
}

/// What `drawing.view.projection_for` can't read is refused, and nothing changes: an offset
/// missing, null, not two numbers, or past 10000 mm either way; a parent that isn't a view, or not
/// a view's id; an unknown field; no drawing open.
#[test]
fn hostile_projection_for_is_refused() {
    let mut s = block();
    let message = s.execute(PROJECTION_FOR, &json!({"parent": ids::encode(1), "offset_mm": [1, 0]})).unwrap_err().to_string();
    assert!(message.contains("drawing"), "{message}");
    let mut s = block_drawing();
    let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": "1:2"}));
    let parent = json!(ids::encode(front));
    for p in [
        json!({"parent": parent}),
        json!({"parent": parent, "offset_mm": null}),
        json!({"parent": parent, "offset_mm": [1]}),
        json!({"parent": parent, "offset_mm": [1, 2, 3]}),
        json!({"parent": parent, "offset_mm": ["a", 0]}),
        json!({"parent": parent, "offset_mm": [1e300, 0]}),
        json!({"parent": parent, "offset_mm": [10000.001, 0]}),
        json!({"parent": parent, "offset_mm": [0, -10000.001]}),
        json!({"parent": ids::encode(99), "offset_mm": [1, 0]}),
        json!({"parent": 1, "offset_mm": [1, 0]}),
        json!({"parent": parent, "offset_mm": [1, 0], "side": "right"}),
    ] {
        create_refused(&mut s, PROJECTION_FOR, p);
    }
}

/// A pictorial view made from what can't take one is refused whole: a side that isn't one of the
/// eight, a pictorial parent, and a 33rd view in a chain.
#[test]
fn hostile_pictorial_view_creation_is_refused() {
    let mut s = block_drawing();
    let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": "1:2"}));
    let parent = json!(ids::encode(front));
    for side in ["above-right", "aboveright", "diagonal", "top"] {
        create_refused(&mut s, PROJECTED, json!({"parent": parent, "side": side}));
    }
    let (_, iso) = create(&mut s, PROJECTED, json!({"parent": parent, "side": "above_right"}));
    for side in ["right", "below_left"] {
        let message = create_refused(&mut s, PROJECTED, json!({"parent": ids::encode(iso), "side": side}));
        assert!(message.contains("not supported yet: a view projected from a pictorial view"), "{message}");
    }
    let mut last = front;
    for _ in 1..32 {
        last = create(&mut s, PROJECTED, json!({"parent": ids::encode(last), "side": "above", "place": {"at": [100.0, 100.0]}})).1;
    }
    let message = create_refused(&mut s, PROJECTED, json!({"parent": ids::encode(last), "side": "above_right"}));
    assert!(message.contains("at most 32"), "{message}");
}

/// A pictorial view copied alone is a base view looking the way it did, saved by its name; the
/// drawing lists it after its parent with its diagonal and its label; deleting its parent's sheet
/// is refused while it is on another sheet.
#[test]
fn a_pictorial_view_copies_lists_and_keeps_its_parents_sheet() {
    let (mut s, [front, right, top]) = three_views();
    let (made, iso) = create(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": "above_right", "place": {"at": [60.0, 250.0]}}));
    assert!(made.get("letter").is_none(), "nothing names a pictorial view by a letter: {made}");
    let second = second_sheet(&mut s);
    let copied = to_sheet(&mut s, COPY, json!({"view": ids::encode(iso), "sheet": ids::encode(second)}));
    let copy = ids::decode(copied["view"].as_str().unwrap()).unwrap();
    assert!(
        matches!(saved(&s, copy).kind, ViewKind::Base { view: ViewDirection::Named(NamedView::IsoFrontRight), .. }),
        "{:?}",
        saved(&s, copy).kind
    );

    let listed = run(&mut s, "drawing.inspect", json!({}));
    let views = listed["sheets"][0]["views"].as_array().unwrap();
    let entry = views.iter().find(|v| v["id"] == json!(ids::encode(iso))).unwrap();
    assert_eq!(
        (&entry["kind"], &entry["parent"], &entry["side"], &entry["label"]),
        (&json!("pictorial"), &json!(ids::encode(front)), &json!("above_right"), &json!("Pictorial above right of Front"))
    );
    let at = |id: u64| views.iter().position(|v| v["id"] == json!(ids::encode(id))).unwrap();
    assert!(at(front) < at(iso), "after its parent");
    assert!([right, top].iter().all(|v| views.iter().any(|e| e["id"] == json!(ids::encode(*v)))));

    run(&mut s, MOVE, json!({"view": ids::encode(iso), "sheet": ids::encode(second)}));
    let sheet = ids::encode(saved(&s, front).sheet);
    let message = s.execute("drawing.sheet.delete", &json!({"sheet": sheet, "contents": "delete"})).unwrap_err().to_string();
    assert!(message.contains("projected or cut from them on other sheets"), "{message}");
}

/// The block with its top front edge chamfered 10√3 along the top and 10 down the front, so the new
/// face slopes 30° from the top and is 20 long up the slope; its front view at `scale` and the right
/// view lined up with it, which shows the slope as a line. The session and the two views.
fn sloped(scale: &str) -> (Session, [u64; 2]) {
    let mut s = block();
    run(&mut s, "solid.chamfer", json!({"edges": [[50, 0, 20]], "distance": 10.0 * 3f64.sqrt(), "distance2": 10, "body": "Block"}));
    run(&mut s, "drawing.create", json!({}));
    let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": scale, "position_mm": [120.0, 120.0]}));
    let (_, right) = create(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": "right"}));
    (s, [front, right])
}

/// The slope's corners on the block's end at `x`: its foot on the front face and its top on the top.
fn slope_foot(x: f64) -> Vec3 {
    Vec3::new(x, 0.0, 10.0)
}

fn slope_top(x: f64) -> Vec3 {
    Vec3::new(x, 10.0 * 3f64.sqrt(), 20.0)
}

/// A point inside the slope's edge on the block's right end, to pick it by.
fn slope_edge() -> Value {
    let middle = (slope_foot(100.0) + slope_top(100.0)) * 0.5;
    json!([middle.x, middle.y, middle.z])
}

/// Where view `id` puts the model point `p` on its sheet: the one rule its lines are placed by.
fn on_paper(s: &Session, id: u64, p: Vec3) -> Vec2 {
    let d = s.drawing("test").unwrap();
    let resolved = d.resolve_view(id).unwrap();
    let placed = crate::drawing_view::on_sheet(s, d, id, "test").unwrap();
    let centre = d.view_centre(id).unwrap();
    crate::drawing_view::frame(resolved.basis, resolved.scale.value, &placed.extents.model, centre, "test").unwrap().to_paper(p)
}

/// An auxiliary view of the 30° slope, made from the right view by the slope's edge on the block's
/// right end, shows the slope at its true length, 20 (the front view shows only its 10 rise), and
/// the slope's other side at its 100, square to it: the face at its true shape, measured on paper at
/// 1:1 and 1:2. It sits up and left of the right view, away from its middle, at its scale. Made by
/// the sloped face itself, it is the same view.
#[test]
fn an_auxiliary_view_of_a_30_degree_slope_shows_its_true_length() {
    for (scale, factor) in [("1:1", 1.0), ("1:2", 0.5)] {
        let (mut s, [front, right]) = sloped(scale);
        let (made, aux) = create(&mut s, AUXILIARY, json!({"parent": ids::encode(right), "edge": slope_edge()}));
        assert_eq!(made["scale"]["text"], scale);
        let apart = |s: &Session, view: u64, a: Vec3, b: Vec3| (on_paper(s, view, a) - on_paper(s, view, b)).len();
        let up_the_slope = apart(&s, aux, slope_foot(100.0), slope_top(100.0));
        assert!((up_the_slope - 20.0 * factor).abs() < EPS, "{scale}: the slope's true length, {up_the_slope}");
        assert!((apart(&s, aux, slope_foot(0.0), slope_foot(100.0)) - 100.0 * factor).abs() < EPS, "{scale}: its other side");
        let foot = on_paper(&s, aux, slope_foot(100.0));
        let (up, across) = (on_paper(&s, aux, slope_top(100.0)) - foot, on_paper(&s, aux, slope_foot(0.0)) - foot);
        assert!((up.x * across.x + up.y * across.y).abs() < EPS, "{scale}: square, the face's true shape");
        assert!((apart(&s, front, slope_foot(100.0), slope_top(100.0)) - 10.0 * factor).abs() < EPS, "{scale}: the front view shows its rise");
        let ViewKind::Auxiliary { parent, toward } = saved(&s, aux).kind else { panic!("{:?}", saved(&s, aux).kind) };
        assert_eq!(parent, right);
        assert!((toward[0] + 0.5).abs() < EPS && (toward[1] - 3f64.sqrt() / 2.0).abs() < EPS, "up and left: {toward:?}");
        let face = (slope_foot(50.0) + slope_top(50.0)) * 0.5;
        let (_, by_face) = create(&mut s, AUXILIARY, json!({"parent": ids::encode(right), "face": [face.x, face.y, face.z]}));
        let ViewKind::Auxiliary { toward: same, .. } = saved(&s, by_face).kind else { panic!("{:?}", saved(&s, by_face).kind) };
        assert!((same[0] - toward[0]).abs() < EPS && (same[1] - toward[1]).abs() < EPS, "{same:?} {toward:?}");
    }
}

/// An auxiliary view goes where its parent unfolds to: every corner of the block keeps its place
/// along the fold line (the line the slope's edge shows as in the right view), so each lies on a
/// projector square to it; and it is moved along its side until its boundary is 20 mm past its
/// parent's on the nearer axis. Toward the other side it goes there; a point given is kept. Folded
/// right of the front view it is the right view, placed where the projected view goes.
#[test]
fn an_auxiliary_view_lines_up_a_gap_past_its_parent() {
    let (mut s, [front, right]) = sloped("1:2");
    let (made, aux) = create(&mut s, AUXILIARY, json!({"parent": ids::encode(right), "edge": slope_edge()}));
    let fold = Vec2::new(3f64.sqrt() / 2.0, 0.5);
    for i in 0..8 {
        let p = Vec3::new(f64::from(i & 1) * 100.0, f64::from((i >> 1) & 1) * 50.0, f64::from((i >> 2) & 1) * 20.0);
        let projector = on_paper(&s, aux, p) - on_paper(&s, right, p);
        assert!((projector.x * fold.x + projector.y * fold.y).abs() < EPS, "{p:?}: {projector:?}");
    }
    let theirs = inspect_view(&mut s, right);
    let (their_lo, their_hi) = (point(&theirs["boundary_mm"]["min"]), point(&theirs["boundary_mm"]["max"]));
    let (lo, hi) = (point(&made["boundary_mm"]["min"]), point(&made["boundary_mm"]["max"]));
    // Up and left: past the parent's top, or its left side, whichever it clears first.
    let gap = (lo.y - their_hi.y).max(their_lo.x - hi.x);
    assert!((gap - 20.0).abs() < EPS, "{gap}: {made}");

    let p = json!({"parent": ids::encode(right), "edge": slope_edge(), "toward": [0.5, -1.0]});
    let (_, other) = create(&mut s, AUXILIARY, p);
    let ViewKind::Auxiliary { toward, .. } = saved(&s, other).kind else { panic!("{:?}", saved(&s, other).kind) };
    assert!((toward[0] - 0.5).abs() < EPS && (toward[1] + 3f64.sqrt() / 2.0).abs() < EPS, "down and right: {toward:?}");
    let p = json!({"parent": ids::encode(right), "edge": slope_edge(), "place": {"at": [300.0, 60.0]}});
    let (_, apart) = create(&mut s, AUXILIARY, p);
    assert_eq!(saved(&s, apart).place, ViewPlace::At([300.0, 60.0]));

    // The block's back right upright edge is a vertical line in the front view: folded to its right.
    let p = json!({"parent": ids::encode(front), "edge": [100.0, 50.0, 10.0], "toward": [1.0, 0.0]});
    let (folded, folded_id) = create(&mut s, AUXILIARY, p);
    let d = s.drawing("test").unwrap().clone();
    assert_eq!(d.resolve_view(folded_id).unwrap().basis, d.resolve_view(right).unwrap().basis);
    let (lo, hi) = (point(&folded["boundary_mm"]["min"]), point(&folded["boundary_mm"]["max"]));
    assert!((lo - their_lo).len() < EPS && (hi - their_hi).len() < EPS, "{folded} {theirs}");
}

/// What `drawing.view.auxiliary` refuses, each with the drawing and the undo steps unchanged: no edge
/// or face, or both; an edge the parent sees end-on; a face it doesn't see edge-on, with its angle; a
/// side along the fold line or of no length; a place lined up; no edge at the point; badly given
/// fields; an edge that isn't straight and a face that isn't flat; a pictorial parent.
#[test]
fn hostile_auxiliary_view_creation_is_refused() {
    let (mut s, [front, right]) = sloped("1:2");
    let parent = json!(ids::encode(right));
    let face = (slope_foot(50.0) + slope_top(50.0)) * 0.5;
    for (p, says) in [
        (json!({"parent": parent}), "one of them"),
        (json!({"parent": parent, "edge": slope_edge(), "face": [face.x, face.y, face.z]}), "one of them"),
        (json!({"parent": parent, "edge": [50.0, 50.0, 20.0]}), "end-on"),
        (json!({"parent": ids::encode(front), "face": [50.0, 0.0, 5.0]}), "90.00°"),
        (json!({"parent": parent, "edge": slope_edge(), "toward": [3f64.sqrt() / 2.0, 0.5]}), "along the fold line"),
        (json!({"parent": parent, "edge": slope_edge(), "toward": [0, 0]}), "no length"),
        (json!({"parent": parent, "edge": slope_edge(), "place": {"aligned": 10.0}}), "an auxiliary view is placed at a point"),
        (json!({"parent": parent, "edge": [500, 500, 500]}), "no edge"),
        (json!({"parent": parent, "edge": slope_edge(), "toward": [1, 2, 3]}), ""),
        (json!({"parent": parent, "edge": slope_edge(), "toward": null}), ""),
        (json!({"parent": parent, "edge": slope_edge(), "side": "right"}), ""),
        (json!({"parent": ids::encode(99), "edge": slope_edge()}), ""),
        (json!({"parent": parent, "edge": slope_edge(), "place": {"at": [1e300, 0]}}), ""),
    ] {
        let message = create_refused(&mut s, AUXILIARY, p.clone());
        assert!(message.contains(says), "{p}: {message}");
    }
    let (_, iso) = create(&mut s, PROJECTED, json!({"parent": ids::encode(front), "side": "above_right"}));
    let message = create_refused(&mut s, AUXILIARY, json!({"parent": ids::encode(iso), "edge": slope_edge()}));
    assert!(message.contains("not supported yet: a view projected from a pictorial view"), "{message}");

    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 20}));
    run(&mut s, "drawing.create", json!({}));
    let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": "1:1"}));
    let rim = create_refused(&mut s, AUXILIARY, json!({"parent": ids::encode(front), "edge": [0.0, 10.0, 20.0]}));
    assert!(rim.contains("isn't straight"), "{rim}");
    let side = create_refused(&mut s, AUXILIARY, json!({"parent": ids::encode(front), "face": [0.0, 10.0, 10.0]}));
    assert!(side.contains("isn't flat"), "{side}");
}

/// An auxiliary view is inspected with its kind, its parent and its side, looking along the slope's
/// normal; the drawing lists it after its parent, with its parent and its label; copied alone it is
/// a base view looking the way it did.
#[test]
fn an_auxiliary_view_inspects_lists_and_copies() {
    let (mut s, [front, right]) = sloped("1:2");
    let (made, aux) = create(&mut s, AUXILIARY, json!({"parent": ids::encode(right), "edge": slope_edge()}));
    assert_eq!(made["letter"], "A", "placed apart, it is named by its letter: {made}");
    let inspected = inspect_view(&mut s, aux);
    assert_eq!((&inspected["kind"], &inspected["parent"], inspected.get("side")), (&json!("auxiliary"), &json!(ids::encode(right)), None));
    let toward = point(&inspected["toward"]);
    assert!((toward - Vec2::new(-0.5, 3f64.sqrt() / 2.0)).len() < EPS, "{inspected}");
    let sight = Vec3::new(0.0, 0.5, -3f64.sqrt() / 2.0);
    let looking = &inspected["direction"]["direction"];
    let looking = Vec3::new(looking[0].as_f64().unwrap(), looking[1].as_f64().unwrap(), looking[2].as_f64().unwrap());
    assert!((looking - sight).len() < EPS, "into the slope: {inspected}");
    assert_eq!((&inspected["settings"]["scale"]["from"], &inspected["linked"]), (&json!("parent"), &json!(true)));

    let listed = run(&mut s, "drawing.inspect", json!({}));
    let views = listed["sheets"][0]["views"].as_array().unwrap();
    let entry = views.iter().find(|v| v["id"] == json!(ids::encode(aux))).unwrap();
    assert_eq!(
        (&entry["kind"], &entry["parent"], entry.get("side"), &entry["label"]),
        (&json!("auxiliary"), &json!(ids::encode(right)), None, &json!("Auxiliary of Right of Front"))
    );
    let at = |id: u64| views.iter().position(|v| v["id"] == json!(ids::encode(id))).unwrap();
    assert!(at(front) < at(right) && at(right) < at(aux), "after its parent");

    let copied = to_sheet(&mut s, COPY, json!({"view": ids::encode(aux), "sheet": "Sheet 1"}));
    let copy = ids::decode(copied["view"].as_str().unwrap()).unwrap();
    assert!(matches!(saved(&s, copy).kind, ViewKind::Base { view: ViewDirection::Looking(_), .. }), "{:?}", saved(&s, copy).kind);
    let d = s.drawing("test").unwrap().clone();
    let (was, is) = (d.resolve_view(aux).unwrap().basis, d.resolve_view(copy).unwrap().basis);
    for (a, b) in [(was.x, is.x), (was.y, is.y), (was.origin, is.origin)] {
        assert!((a - b).len() < 1e-12, "{a:?} {b:?}");
    }
}

/// Each quarter turn turns the flat on the sheet: at 90 and 270 degrees its boundary is the flat's
/// height wide and its width high. On its plane, each turn puts the flat's corners exactly where
/// that turn anticlockwise takes them, so 90 and 270 degrees can't be told apart only by size.
#[test]
fn a_quarter_turn_turns_a_flat_view() {
    let mut s = sheet_plate();
    let reply = flat_pattern(&mut s);
    let turned = |turn| crate::drawing_view::flat_from(&reply, turn, FLAT).unwrap().model;
    let flat = turned(QuarterTurn::None);
    // A box centred on the plane's origin would turn to the same box at 90 and 270 degrees.
    assert!(flat.min.x != -flat.max.x || flat.min.y != -flat.max.y, "{flat:?}");
    let corners = [flat.min, Vec2::new(flat.max.x, flat.min.y), flat.max, Vec2::new(flat.min.x, flat.max.y)];
    let rules: [(QuarterTurn, fn(Vec2) -> Vec2); 3] = [
        (QuarterTurn::Quarter, |p| Vec2::new(-p.y, p.x)),
        (QuarterTurn::Half, |p| Vec2::new(-p.x, -p.y)),
        (QuarterTurn::ThreeQuarters, |p| Vec2::new(p.y, -p.x)),
    ];
    for (turn, rule) in rules {
        let at = corners.map(rule);
        let min = at.iter().fold(Vec2::new(f64::INFINITY, f64::INFINITY), |m, p| Vec2::new(m.x.min(p.x), m.y.min(p.y)));
        let max = at.iter().fold(Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY), |m, p| Vec2::new(m.x.max(p.x), m.y.max(p.y)));
        let got = turned(turn);
        assert!(got.min == min && got.max == max, "{turn:?}: {got:?}, want {min:?} to {max:?}");
    }
    let size = flat_size(&mut s);
    for (turn, want) in [(0, size), (90, Vec2::new(size.y, size.x)), (180, size), (270, Vec2::new(size.y, size.x))] {
        let (_, reply) = flat_view(&mut s, json!({"sheet": "Sheet 1", "scale": "1:1", "turn": turn}));
        assert!((boundary(&reply).0 - want).len() < 1e-9, "{turn}: {reply}");
    }
    let error = s.execute(FLAT, &json!({"sheet": "Sheet 1", "scale": "1:1", "turn": 45})).unwrap_err().to_string();
    assert!(error.contains("0, 90, 180 or 270"), "{error}");
}

/// A body that isn't sheet metal is refused in `sheet.flat_pattern`'s words, and so is a design with
/// none. A body renamed after its flat view was made fails that view with the reason, and the sheet
/// still draws.
#[test]
fn a_flat_view_of_a_body_that_isnt_sheet_metal_is_refused() {
    let mut s = block();
    run(&mut s, "drawing.create", json!({}));
    let error = s.execute(FLAT, &json!({"sheet": "Sheet 1", "body": "Block"})).unwrap_err().to_string();
    assert!(error.contains("`Block` is not a sheet metal body"), "{error}");
    let error = s.execute(FLAT, &json!({"sheet": "Sheet 1"})).unwrap_err().to_string();
    assert!(error.contains("there is no sheet metal body"), "{error}");

    let mut s = sheet_plate();
    let (id, _) = flat_view(&mut s, json!({"sheet": "Sheet 1", "body": "Plate", "scale": "1:1"}));
    with_the_model(&mut s, |s| {
        run(s, "body.rename", json!({"body": "Plate", "name": "Blank"}));
    });
    let (strokes, status) = flat_strokes(&s, id);
    assert!(strokes.is_empty() && status.reason().is_some_and(|why| why.contains("`Plate` is not a sheet metal body")), "{status:?}");
}

/// A flat view follows the model: a taller flange and a smaller K-factor each change its size by what
/// the flat pattern's size changes.
#[test]
fn a_flat_view_follows_the_flange_and_the_rule() {
    let mut s = sheet_plate();
    let (id, _) = flat_view(&mut s, json!({"sheet": "Sheet 1", "scale": "1:1"}));
    let size_on_sheet = |s: &mut Session| boundary(&inspect_view(s, id)).0;
    assert!((size_on_sheet(&mut s) - flat_size(&mut s)).len() < 1e-6);
    let (mut taller, mut smaller) = (Vec2::ZERO, Vec2::ZERO);
    with_the_model(&mut s, |s| {
        let names = s.doc.features.last().map(|f| f.param_names.clone()).unwrap_or_default();
        run(s, "parameters.change", json!({"name": names[0], "expression": "30"}));
        taller = flat_size(s);
        run(s, "sheet.manage_rules", json!({"name": "T2", "k_factor": 0.3}));
        smaller = flat_size(s);
    });
    assert!(taller.y > 69.0 && smaller.y < taller.y, "{taller:?} {smaller:?}");
    assert!((size_on_sheet(&mut s) - smaller).len() < 1e-6);
}

/// A flat view inspects as `flat`, and the drawing's status answers with it on the sheet. No view of
/// any kind is made from it yet: each command refuses with nothing changed, and
/// `drawing.view.projection_for` gives none with the same reason.
#[test]
fn a_flat_view_inspects_as_flat() {
    let mut s = sheet_plate();
    let (id, _) = flat_view(&mut s, json!({"sheet": "Sheet 1", "scale": "1:1"}));
    assert_eq!(inspect_view(&mut s, id)["kind"], json!("flat"));
    run(&mut s, "drawing.status", json!({}));
    let (parent, refusal) = (ids::encode(id), "not supported yet: a view made from a flat pattern view");
    for (cmd, p) in [
        (PROJECTED, json!({"parent": parent, "side": "right"})),
        (PROJECTED, json!({"parent": parent, "side": "above_right"})),
        (AUXILIARY, json!({"parent": parent, "edge": [50, 60, 2]})),
    ] {
        let message = create_refused(&mut s, cmd, p);
        assert!(message.contains(refusal), "{cmd}: {message}");
    }
    let at = run(&mut s, PROJECTION_FOR, json!({"parent": parent, "offset_mm": [80.0, 0.0]}));
    assert_eq!((at["kind"].as_str(), at["why"].as_str()), (Some("none"), Some(refusal)));
}

/// A flat view saved with its design and opened again in a new session keeps its body and turn, and
/// draws the same lines.
#[test]
fn a_flat_view_is_the_same_after_saving_and_opening() {
    let mut s = sheet_plate();
    let (id, _) = flat_view(&mut s, json!({"sheet": "Sheet 1", "scale": "1:1", "turn": 90}));
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    let dir = std::env::temp_dir().join(format!("solidcraft-flat-view-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("part.{}", solidcraft_io::DESIGN_EXT));
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": path}));
    run(&mut opened, "drawing.open", drawing);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(opened.drawing("test").unwrap().view(id).unwrap().kind, s.drawing("test").unwrap().view(id).unwrap().kind);
    let ((lines, status), (was, before)) = (flat_strokes(&opened, id), flat_strokes(&s, id));
    assert_eq!(status.name(), "drawn", "{status:?}");
    assert_eq!((lines, status.name()), (was, before.name()));
}

/// A drawn full section keeps a centre line on its drawn parent.
#[test]
fn a_drawn_section_keeps_a_parent_views_centre_line() {
    let (mut s, [front, ..]) = three_views();
    let cut = with_section(&mut s, front);
    let added = run(&mut s, "drawing.center_line.add", json!({"view": ids::encode(front), "a": [0, 0, 0], "b": [100, 0, 0], "overshoot_mm": 2.0}));
    let line = ids::decode(added["line"].as_str().unwrap()).unwrap();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let scene = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    let state = scene["views"].as_array().unwrap().iter().find(|v| v["view"] == json!(ids::encode(cut))).unwrap();
    assert_eq!(state["status"], "drawn", "{state}");
    assert!(state["reason"].is_null(), "{state}");
    assert!(scene["scene"]["groups"].as_array().unwrap().iter().any(|g| g["object"] == json!(line) && g["kind"] == "centre_line"));
}

/// `drawing.view.section` makes a section view of a saved view along the cutting line it is given,
/// in one undo step: it saves that line and the place given, follows its parent's settings, and
/// draws as the same view written into the drawing by hand does.
#[test]
fn a_section_view_is_made_along_its_line() {
    use solidcraft_doc::drawing::cutting_line::{ArrowSide, CuttingLine, SectionKind};
    let (mut s, [front, ..], by_hand) = drilled_with_tab();
    let line = json!({"kind": "full", "points": [[50.0, -10.0], [50.0, 70.0]], "arrows": "right"});
    let (made, id) = create(&mut s, SECTION, json!({"parent": ids::encode(front), "line": line, "place": {"at": [150.0, 60.0]}}));
    let expected = CuttingLine::new(SectionKind::Full, vec![Vec2::new(50.0, -10.0), Vec2::new(50.0, 70.0)], ArrowSide::Right).unwrap();
    let view = saved(&s, id);
    assert_eq!(
        (view.kind, view.place, view.settings),
        (ViewKind::Section { parent: front, line: expected }, ViewPlace::At([150.0, 60.0]), ViewOverrides::default())
    );
    assert_eq!(made["scale"]["text"], inspect_view(&mut s, front)["settings"]["scale"]["value"], "{made}");
    let ((_, _, ours), (_, _, theirs)) = (drawn_lines(&s, id), drawn_lines(&s, by_hand));
    for visibility in [Visibility::Visible, Visibility::Hidden] {
        assert_eq!(upright(&ours, visibility), upright(&theirs, visibility), "{visibility:?}");
    }
}

/// With no place, a section view sits where a projected view looking its way would: lined up with
/// its parent, its boundary a gap past the parent's, on the side the drawing's projection gives. On
/// the block's front view at 1:2 (185..235 x 143.5..153.5), a line across it with its arrows down
/// looks down on the half below the line, 50 x 25 on paper: above the parent in third angle, where
/// the top view goes, and below it in first. An upright line with its arrows right looks at the
/// right half from the left, 25 x 10: left of the parent in third angle.
#[test]
fn a_section_view_sits_where_the_projection_puts_it() {
    let mut s = block_drawing();
    let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": "1:2"}));
    let section = |s: &mut Session, points: Value| {
        create(s, SECTION, json!({"parent": ids::encode(front), "line": {"kind": "full", "points": points, "arrows": "right"}}))
    };
    let across = json!([[-10.0, 10.0], [110.0, 10.0]]);
    let (above, above_id) = section(&mut s, across.clone());
    assert_box(&above, [185.0, 173.5], [235.0, 198.5]);
    assert_eq!(saved(&s, above_id).place, ViewPlace::At([210.0, 186.0]));
    assert_box(&section(&mut s, json!([[50.0, -10.0], [50.0, 30.0]])).0, [140.0, 143.5], [165.0, 153.5]);
    set_projection(&mut s, Projection::First);
    assert_box(&section(&mut s, across).0, [185.0, 98.5], [235.0, 123.5]);
}

/// What the drawing would refuse is refused before anything is saved, naming the command: a line
/// with one point, two points on top of each other, a point past 1e12 mm, a half line that doesn't
/// turn, a parent that isn't a view, and a place lined up with the parent.
#[test]
fn a_bad_cutting_line_is_refused() {
    let mut s = block_drawing();
    let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": "1:2"}));
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let made =
        |parent: u64, kind: &str, points: Value| json!({"parent": ids::encode(parent), "line": {"kind": kind, "points": points, "arrows": "left"}});
    let mut aligned = made(front, "full", json!([[0.0, 10.0], [100.0, 10.0]]));
    aligned["place"] = json!({"aligned": 40.0});
    for bad in [
        made(front, "full", json!([[0.0, 0.0]])),
        made(front, "full", json!([[5.0, 5.0], [5.0, 5.0]])),
        made(front, "full", json!([[0.0, 0.0], [1e13, 0.0]])),
        made(front, "half", json!([[0.0, 0.0], [10.0, 0.0], [20.0, 0.0]])),
        made(sheet, "full", json!([[0.0, 10.0], [100.0, 10.0]])),
        aligned,
    ] {
        let why = create_refused(&mut s, SECTION, bad.clone());
        assert!(why.contains(SECTION), "{bad}: {why}");
    }
}

/// Section kinds that can't be cut yet (`half`, `offset` and `aligned`) and a section of a shaded
/// parent are refused by `drawing.view.section` before anything is saved, so they never leave a
/// failed view on the sheet that stops `drawing.export` (#19879).
#[test]
fn unsupported_section_views_are_refused_and_keep_the_sheet_exportable() {
    let mut s = block_drawing();
    let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": "1:2"}));
    for (kind, points, expected) in [
        ("half", json!([[-10.0, 10.0], [50.0, 10.0], [50.0, 30.0]]), "not supported yet: a half section"),
        ("offset", json!([[-10.0, 5.0], [50.0, 5.0], [50.0, 15.0], [110.0, 15.0]]), "not supported yet: an offset section"),
        ("aligned", json!([[-10.0, 10.0], [50.0, 10.0], [80.0, 30.0]]), "not supported yet: an aligned section"),
    ] {
        let params = json!({
            "parent": ids::encode(front),
            "line": {"kind": kind, "points": points, "arrows": "right"},
            "place": {"at": [300.0, 60.0]},
        });
        let why = create_refused(&mut s, SECTION, params);
        assert!(why.contains(SECTION) && why.contains(expected), "{kind}: {why}");
    }
    let exported = run(&mut s, "drawing.export", json!({"sheet": "Sheet 1"}));
    assert_eq!(exported["views"].as_array().map(Vec::len), Some(1), "{exported}");
    assert_eq!(exported["views"][0]["status"], json!("drawn"), "{exported}");

    // A full section of a shaded parent would inherit the shaded style, which isn't cut yet either.
    edit_view(&mut s, front, json!({"visual_style": "shaded"}));
    let shaded = create_refused(
        &mut s,
        SECTION,
        json!({"parent": ids::encode(front), "line": {"kind": "full", "points": [[-10.0, 10.0], [110.0, 10.0]], "arrows": "right"}}),
    );
    assert!(shaded.contains(SECTION) && shaded.contains("not supported yet: a shaded section view"), "{shaded}");
    let exported = run(&mut s, "drawing.export", json!({"sheet": "Sheet 1"}));
    assert_eq!(exported["views"].as_array().map(Vec::len), Some(1), "{exported}");
    assert_eq!(exported["views"][0]["status"], json!("drawn"), "{exported}");
}

/// A section view takes its letter as it is made, the next one free: A for the first, then B.
#[test]
fn a_section_views_letter_is_given() {
    let mut s = block_drawing();
    let (_, front) = create(&mut s, BASE, json!({"sheet": "Sheet 1", "scale": "1:2"}));
    let line = json!({"kind": "full", "points": [[-10.0, 10.0], [110.0, 10.0]], "arrows": "right"});
    let mut letters = Vec::new();
    for at in [[100.0, 60.0], [300.0, 60.0]] {
        let (made, id) = create(&mut s, SECTION, json!({"parent": ids::encode(front), "line": line, "place": {"at": at}}));
        let saved_letter = saved(&s, id).label.map(|label| label.letter);
        assert_eq!(made["letter"].as_str(), saved_letter.as_deref(), "{made}");
        letters.extend(saved_letter);
    }
    assert_eq!(letters, ["A", "B"]);
}

/// A 40 x 40 x 20 mm block from the origin with an M8 tapped hole down from the middle of its top:
/// drilled Ø6.8 16 deep, threaded 12 deep. At 1:1 the block's centre is the A3 sheet's (210, 148.5),
/// so from above the hole is round it, and from the front its top is at y = 158.5 on the sheet.
fn tapped_block() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 20, "body_name": "Block"}));
    run(&mut s, "solid.hole", json!({"position": [20, 20, 20], "direction": [0, 0, -1], "thread": "M8", "depth": 16, "thread_depth": 12}));
    s
}

/// Every segment of `v`'s thread pieces of `stroke` at `visibility`, on the sheet.
fn thread_segs(v: &Value, stroke: &str, visibility: &str) -> Vec<Value> {
    let pieces = v["threads"].as_array().into_iter().flatten().flat_map(|t| t["pieces"].as_array().into_iter().flatten());
    let wanted = pieces.filter(|p| p["stroke"] == stroke && p["visibility"] == visibility);
    wanted.flat_map(|p| p["path"].as_array().into_iter().flatten().cloned()).collect()
}

/// A line segment's ends, top first.
fn top_first(seg: &Value) -> (Vec2, Vec2) {
    let (a, b) = (point(&seg["a"]), point(&seg["b"]));
    if a.y >= b.y { (a, b) } else { (b, a) }
}

/// From above, the M8 tapped hole draws its root as a thin three-quarter arc of radius 4.0 round
/// the hole's centre, from 90° round to 360°, so its gap is the upper right quadrant: the Ø6.8 drill
/// is the circle the view draws already.
#[test]
fn a_tapped_holes_root_is_a_three_quarter_arc_seen_from_above() {
    let mut s = tapped_block();
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1"}));
    let schema = output_schema_for::<SnapshotResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &v), "{v}");
    let roots = thread_segs(&v, "root", "visible");
    assert_eq!(roots.len(), 1, "{v}");
    let arc = &roots[0];
    assert_eq!(arc["type"], "arc", "{arc}");
    assert!(point(&arc["center"]).dist(Vec2::new(210.0, 148.5)) < 1e-6, "{arc}");
    let (radius, start, sweep) = (arc["radius"].as_f64().unwrap(), arc["start"].as_f64().unwrap(), arc["sweep"].as_f64().unwrap());
    assert!(
        (radius - 4.0).abs() < 1e-6 && (start - std::f64::consts::FRAC_PI_2).abs() < 1e-6 && (sweep - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-6,
        "{arc}"
    );
    assert!(thread_segs(&v, "limit", "visible").is_empty() && thread_segs(&v, "limit", "hidden").is_empty(), "end on, no limit line: {v}");
    assert!(arcs(&v, "visible").iter().any(|(c, r, ..)| c.dist(Vec2::new(210.0, 148.5)) < 1e-6 && (r - 3.4).abs() < 1e-6), "the drill: {v}");
}

/// From the front, with hidden lines drawn, the hole's thread is hidden: two dashed root lines 8.0
/// apart, each 12.0 long down from the top, and a limit line 8.0 long across at 12 deep, where the
/// thread stops in its 16 mm hole. With hidden lines left out, the thread draws nothing.
#[test]
fn a_tapped_holes_thread_is_hidden_from_the_side() {
    let mut s = tapped_block();
    let v = snap(&mut s, json!({"scale": "1:1", "visual_style": "visible_hidden"}));
    let mut roots: Vec<(Vec2, Vec2)> = thread_segs(&v, "root", "hidden").iter().map(top_first).collect();
    roots.sort_by(|p, q| p.0.x.total_cmp(&q.0.x));
    assert_eq!(roots.len(), 2, "{v}");
    for ((top, bottom), x) in roots.iter().zip([206.0, 214.0]) {
        assert!(top.dist(Vec2::new(x, 158.5)) < 1e-6 && bottom.dist(Vec2::new(x, 146.5)) < 1e-6, "{top:?} {bottom:?} {v}");
    }
    let limits: Vec<(Vec2, Vec2)> = thread_segs(&v, "limit", "hidden").iter().map(top_first).collect();
    assert_eq!(limits.len(), 1, "{v}");
    let (a, b) = limits[0];
    assert!(
        (a.dist(b) - 8.0).abs() < 1e-6 && (a.y - 146.5).abs() < 1e-6 && (b.y - 146.5).abs() < 1e-6 && ((a.x + b.x) / 2.0 - 210.0).abs() < 1e-6,
        "{v}"
    );
    assert!(thread_segs(&v, "root", "visible").is_empty() && thread_segs(&v, "limit", "visible").is_empty(), "{v}");
    let v = snap(&mut s, json!({"scale": "1:1"}));
    assert_eq!(v["threads"], json!([]), "{v}");
}

/// A tapped block drawn from the mesh (a mesh cube stands in front of its lower right corner, clear
/// of the hole) still draws its thread: two hidden root lines 8.0 apart, each 12.0 long, and a limit
/// line 8.0 long across their lower ends. The thread lines pair with the mesh's crest lines as with
/// exact ones.
#[test]
fn a_tapped_block_drawn_from_the_mesh_draws_its_thread() {
    let mut s = tapped_block();
    with_mesh_cube(&mut s, [30.0, -30.0, -10.0], "tapped");
    let v = snap(&mut s, json!({"scale": "1:1", "visual_style": "visible_hidden"}));
    assert_eq!(v["status"]["provenance"], "mesh_projection", "{v}");
    let mut roots: Vec<(Vec2, Vec2)> = thread_segs(&v, "root", "hidden").iter().map(top_first).collect();
    roots.sort_by(|p, q| p.0.x.total_cmp(&q.0.x));
    assert_eq!(roots.len(), 2, "{v}");
    let (left, right) = (roots[0], roots[1]);
    assert!((right.0.x - left.0.x - 8.0).abs() < 1e-6 && (left.0.y - right.0.y).abs() < 1e-6, "{roots:?}");
    assert!(roots.iter().all(|(top, bottom)| (top.x - bottom.x).abs() < 1e-6 && (top.y - bottom.y - 12.0).abs() < 1e-6), "{roots:?}");
    let limits: Vec<(Vec2, Vec2)> = thread_segs(&v, "limit", "hidden").iter().map(top_first).collect();
    assert_eq!(limits.len(), 1, "{v}");
    let (a, b) = limits[0];
    assert!((a.dist(b) - 8.0).abs() < 1e-6 && (a.y - left.1.y).abs() < 1e-6 && (b.y - left.1.y).abs() < 1e-6, "{limits:?} {roots:?}");
}

/// An M8 shaft threaded 10 mm down from its top, seen from the side: its root lines are thin and
/// seen, at the minor radius a modelled M8 is cut to (4 - 5H/8 for H = 1.25 √3 / 2), 10 long; a
/// limit line goes across where the thread stops, and none at the free end, where the shaft's own
/// edge is drawn.
#[test]
fn a_shafts_thread_is_seen_from_the_side() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 20, "body_name": "Shaft"}));
    run(&mut s, "solid.thread", json!({"face": [4, 0, 10], "designation": "M8", "length": 10, "from": [4, 0, 20]}));
    let v = snap(&mut s, json!({"scale": "1:1"}));
    let minor = 4.0 - 5.0 * (1.25 * 3f64.sqrt() / 2.0) / 8.0;
    let mut roots: Vec<(Vec2, Vec2)> = thread_segs(&v, "root", "visible").iter().map(top_first).collect();
    roots.sort_by(|p, q| p.0.x.total_cmp(&q.0.x));
    assert_eq!(roots.len(), 2, "{v}");
    for ((top, bottom), x) in roots.iter().zip([210.0 - minor, 210.0 + minor]) {
        assert!(top.dist(Vec2::new(x, 158.5)) < 1e-6 && bottom.dist(Vec2::new(x, 148.5)) < 1e-6, "{top:?} {bottom:?} {v}");
    }
    let limits: Vec<(Vec2, Vec2)> = thread_segs(&v, "limit", "visible").iter().map(top_first).collect();
    assert_eq!(limits.len(), 1, "{v}");
    assert!((limits[0].0.y - 148.5).abs() < 1e-6 && (limits[0].0.dist(limits[0].1) - 8.0).abs() < 1e-6, "{v}");
}

/// `s`'s front view at 1:2 ([`three_views_of`]) and a full section of it along the line through
/// `points` on its plane (x along the model's x, y up its z), its arrows to the right.
fn front_section(s: Session, points: &[[f64; 2]]) -> (Session, u64) {
    use solidcraft_doc::drawing::cutting_line::{ArrowSide, SectionKind};
    let (mut s, [front, ..]) = three_views_of(s);
    let cut = section_of(&mut s, front, SectionKind::Full, points, ArrowSide::Right);
    (s, cut)
}

/// Section view `id`'s hatching as its sheet draws it, on paper round its model's middle as
/// [`drawn_lines`]' lines are, with the bodies it shows.
fn drawn_hatches(s: &Session, id: u64) -> (ShownSet, Vec<crate::drawing_view::BodyHatch>) {
    let view = crate::drawing_view::drawn_view(s, s.drawing("test").unwrap(), id).unwrap();
    let shown = crate::drawing_view::shown(s);
    let (set, extents) = crate::drawing_view::measured(&shown, &view).unwrap();
    let frame = crate::drawing_view::frame(view.basis, view.scale.value, &extents.model, Vec2::ZERO, "test").unwrap();
    let hatching = crate::drawing_view::hatches(&set, view.cut.as_ref().unwrap(), &frame, view.line_group.value);
    assert_eq!(hatching.refused, vec![], "every cut body is hatched");
    assert_eq!(hatching.doubted, vec![], "no body is hatched in doubt");
    (set.into_owned(), hatching.bodies)
}

/// The hatch of the body named `name`.
fn hatch_of<'a>(set: &ShownSet, hatches: &'a [crate::drawing_view::BodyHatch], name: &str) -> &'a crate::drawing_view::BodyHatch {
    hatches.iter().find(|h| set.bodies[h.shown].body == name).unwrap_or_else(|| panic!("{name} isn't hatched"))
}

/// A hatch stroke's two ends.
fn stroke_ends(seg: &solidcore_record::Seg2) -> (Vec2, Vec2) {
    match *seg {
        solidcore_record::Seg2::Line { a, b } => (a, b),
        _ => panic!("a hatch stroke is a straight line: {seg:?}"),
    }
}

/// How far `p` lies from the nearest point of `regions`' outlines, on paper.
fn off_outline(p: Vec2, regions: &[solidcore_record::Region2]) -> f64 {
    let pieces = regions.iter().flat_map(|r| r.loops()).flat_map(|l| &l.segs);
    pieces.map(|seg| p.dist(seg.point_at(seg.closest_param(p)))).fold(f64::INFINITY, f64::min)
}

/// Every point along `hatch`'s lines lies on its cut faces: inside them, or within the hatcher's
/// flattening of their outline.
fn assert_hatching_inside(hatch: &crate::drawing_view::BodyHatch) {
    use crate::drawing_view::FLATTEN_MM;
    for stroke in &hatch.strokes {
        let (a, b) = stroke_ends(stroke);
        for i in 0..=100 {
            let p = a + (b - a) * (f64::from(i) / 100.0);
            let inside = hatch.regions.iter().any(|r| r.contains(p));
            assert!(inside || off_outline(p, &hatch.regions) <= FLATTEN_MM + 1e-9, "a hatch line leaves its cut face at {p:?}");
        }
    }
}

/// The box round a hatch's cut faces on paper.
fn hatch_box(h: &crate::drawing_view::BodyHatch) -> Aabb2 {
    h.regions.iter().flat_map(|r| r.loops()).flat_map(|l| &l.segs).map(|s| s.bounds()).fold(Aabb2::EMPTY, |a, b| a.union(&b))
}

/// Two 20 mm cubes 10 mm apart, cut across by a level line: each cut face is hatched, every stroke
/// inside its own body's face, and the two faces lie 5 mm apart on paper at 1:2 with nothing in
/// the gap (#13046's acceptance: two hatched regions 10 mm apart).
#[test]
fn two_bodies_10_mm_apart_are_each_hatched_and_the_gap_is_not() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [30, 0, 0], "operation": "new", "body_name": "B"}));
    let (s, cut) = front_section(s, &[[-10.0, 10.0], [60.0, 10.0]]);
    let (set, hatches) = drawn_hatches(&s, cut);
    assert_eq!(hatches.len(), 2, "{hatches:?}");
    let (a, b) = (hatch_of(&set, &hatches, "A"), hatch_of(&set, &hatches, "B"));
    let (box_a, box_b) = (hatch_box(a), hatch_box(b));
    for (h, at) in [(a, box_a), (b, box_b)] {
        assert!(!h.filled && h.strokes.len() >= 3, "{h:?}");
        let grown = Aabb2 { min: at.min - Vec2::new(1e-6, 1e-6), max: at.max + Vec2::new(1e-6, 1e-6) };
        assert!(h.strokes.iter().map(stroke_ends).all(|(p, q)| grown.contains(p) && grown.contains(q)), "{h:?}");
    }
    let gap = (box_b.min.x - box_a.max.x).max(box_a.min.x - box_b.max.x).max(box_b.min.y - box_a.max.y).max(box_a.min.y - box_b.max.y);
    assert!((gap - 5.0).abs() < 1e-6, "{box_a:?} {box_b:?}");
}

/// A pin in its bore: the two cut faces touch, so they are hatched differently (by angle or
/// spacing), and a third body added away from them changes neither (#13046's acceptance: touching
/// bodies' hatches stay as they are).
#[test]
fn touching_bodies_are_hatched_apart_and_keep_their_hatches() {
    // The block and pin, with the spare body or without it.
    let looks = |spare: bool| {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 20, "body_name": "Block"}));
        run(&mut s, "solid.cylinder", json!({"diameter": 10, "height": 22, "base": [20, 20, -1], "operation": "cut", "targets": ["Block"]}));
        run(&mut s, "solid.cylinder", json!({"diameter": 10, "height": 20, "base": [20, 20, 0], "operation": "new", "body_name": "Pin"}));
        if spare {
            run(
                &mut s,
                "solid.box",
                json!({"length": 10, "width": 10, "height": 10, "corner": [100, 0, 0], "operation": "new", "body_name": "Spare"}),
            );
        }
        let (s, cut) = front_section(s, &[[20.0, -10.0], [20.0, 30.0]]);
        let (set, hatches) = drawn_hatches(&s, cut);
        let look = |name: &str| {
            let h = hatch_of(&set, &hatches, name);
            (h.angle, h.spacing_mm)
        };
        (look("Block"), look("Pin"))
    };
    let (block, pin) = looks(false);
    assert_ne!(block, pin);
    assert_eq!(looks(true), (block, pin));
}

/// The drilled block cut through its hole: strokes on both sides of the hole and none across it.
/// The hole's sides are 2.5 mm either side of the middle on paper at 1:2.
#[test]
fn a_cut_hole_is_not_hatched() {
    let (s, _, cut) = drilled_with_tab();
    let (set, hatches) = drawn_hatches(&s, cut);
    let block = hatch_of(&set, &hatches, "Block");
    assert_hatching_inside(block);
    let side = |x: f64| {
        if x <= -2.5 + 1e-6 {
            -1
        } else if x >= 2.5 - 1e-6 {
            1
        } else {
            0
        }
    };
    let sides: BTreeSet<i32> =
        block.strokes.iter().map(stroke_ends).map(|(p, q)| (side(p.x), side(q.x))).map(|(p, q)| if p == q { p } else { 0 }).collect();
    assert_eq!(sides, BTreeSet::from([-1, 1]), "{block:?}");
}

/// A 100 x 60 x 10 plate cut through a centred 10 mm bore and a 30 x 20 x 5 pocket.
/// The bore removes -30 < x < -20 throughout the centred section; the pocket removes
/// 5 < x < 35 above y = 0. Every complete stroke must stay out of both open rectangles.
#[test]
fn a_full_section_through_a_bore_and_pocket_hatches_only_material() {
    for (hole, pocket, cut_y) in
        [(true, true, 30.0_f64), (true, true, 29.99), (true, true, 30.01), (true, false, 30.0), (false, true, 30.0), (false, false, 30.0)]
    {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "body_name": "Plaque"}));
        if hole {
            run(&mut s, "solid.hole", json!({"position": [25, 30, 10], "diameter": 10}));
        }
        if pocket {
            run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 5, "corner": [55, 20, 5], "operation": "cut"}));
        }
        run(&mut s, "drawing.create", json!({}));
        let base = run(&mut s, "drawing.view.base", json!({"sheet": "Sheet 1", "view": "top", "scale": "1:1", "position_mm": [90, 220]}));
        let cut = run(
            &mut s,
            "drawing.view.section",
            json!({"parent": base["view"], "line": {"kind": "full", "points": [[-10, cut_y], [110, cut_y]], "arrows": "left"}}),
        );
        let id = ids::decode(cut["view"].as_str().unwrap()).unwrap();
        let (set, hatches) = drawn_hatches(&s, id);
        let hatch = hatch_of(&set, &hatches, "Plaque");
        assert!(!hatch.filled && !hatch.strokes.is_empty(), "hole={hole}, pocket={pocket}, y={cut_y}: {hatch:?}");
        let hole_half = (25.0 - (cut_y - 30.0).powi(2)).sqrt();
        let mut left = false;
        let mut right = false;
        let mut below_pocket = false;
        for stroke in &hatch.strokes {
            let (a, b) = stroke_ends(stroke);
            let (a, b) = if a.x <= b.x { (a, b) } else { (b, a) };
            assert!(a.x < b.x && a.y >= -5.0 - EPS && b.y <= 5.0 + EPS, "{stroke:?}");
            if hole {
                assert!(b.x <= -25.0 - hole_half + EPS || a.x >= -25.0 + hole_half - EPS, "a stroke crosses the bore: {stroke:?}");
            }
            if pocket {
                let lo = a.x.max(5.0 + EPS);
                let hi = b.x.min(35.0 - EPS);
                if lo < hi {
                    let y = |x| a.y + (b.y - a.y) * ((x - a.x) / (b.x - a.x));
                    assert!(y(lo).max(y(hi)) <= EPS || y(lo).min(y(hi)) >= 5.0 - EPS, "a stroke crosses the pocket: {stroke:?}");
                    below_pocket |= y(lo).min(y(hi)) < -1.0;
                }
            }
            left |= b.x < -30.0;
            right |= a.x > 35.0;
        }
        assert!(left && right && (!pocket || below_pocket), "material on both sides and below the pocket is hatched");
        let scene = run(&mut s, "drawing.scene", json!({"sheet": "Sheet 1"}));
        let state = scene["views"].as_array().unwrap().iter().find(|v| v["view"] == cut["view"]).unwrap();
        assert!(state.get("reason").is_none(), "{state}");
        assert_eq!(state["hatches"][0]["filled"], false, "{state}");
        assert_eq!(state["hatches"][0]["strokes"], hatch.strokes.len(), "{state}");
    }
}

/// The tapped block cut through its hole's axis (#13051's section): the drill's sides are cut edges
/// 1.7 mm either side of the middle at 1:2 (Ø6.8), the thread's root lines are thin and seen at
/// 2.0 (Ø8), 6 mm down from the top (12 deep), with a limit line across; the hatching stops at the
/// drill, so the root lines lie on it, as ISO 6410 draws a cut internal thread.
#[test]
fn a_cut_tapped_hole_draws_its_thread_over_the_hatching() {
    use solidcraft_doc::drawing::thread_lines::ThreadStroke;
    let (s, cut) = front_section(tapped_block(), &[[20.0, -10.0], [20.0, 30.0]]);
    let (_, _, lines) = drawn_lines(&s, cut);
    let edges = upright(&lines, Visibility::Visible);
    assert!(BTreeSet::from([-170, 170]).is_subset(&edges), "{edges:?}");
    let pieces: Vec<_> = lines.threads.iter().flat_map(|t| &t.pieces).collect();
    let roots: BTreeSet<i64> = pieces
        .iter()
        .filter(|p| p.stroke == ThreadStroke::Root && p.visibility == Visibility::Visible)
        .map(|p| stroke_ends(&p.seg))
        .map(|(a, b)| {
            assert!((a.x - b.x).abs() < 1e-9 && ((a.y - b.y).abs() - 6.0).abs() < 1e-9, "{a:?} {b:?}");
            (a.x * 100.0).round() as i64
        })
        .collect();
    assert_eq!(roots, BTreeSet::from([-200, 200]));
    assert_eq!(pieces.iter().filter(|p| p.stroke == ThreadStroke::Limit && p.visibility == Visibility::Visible).count(), 1);
    let (set, hatches) = drawn_hatches(&s, cut);
    let block = hatch_of(&set, &hatches, "Block");
    assert_hatching_inside(block);
    let ends: Vec<(Vec2, Vec2)> = block.strokes.iter().map(stroke_ends).collect();
    assert!(ends.iter().any(|(p, q)| p.x.min(q.x) < 2.0 && p.x.max(q.x) > 2.0), "the root line lies on the hatching: {ends:?}");
}

/// A plate 0.5 mm thick, cut across: its cut face is 0.25 mm wide on paper at 1:2, too thin for a
/// hatch line, so it is painted solid.
#[test]
fn a_thin_cut_face_is_filled() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 0.5, "body_name": "Plate"}));
    let (s, cut) = front_section(s, &[[20.0, -10.0], [20.0, 10.0]]);
    let (set, hatches) = drawn_hatches(&s, cut);
    let plate = hatch_of(&set, &hatches, "Plate");
    assert!(plate.filled && plate.strokes.is_empty() && plate.regions.len() == 1, "{plate:?}");
}

/// Only the bodies the section cuts are hatched: not a mesh body, which is drawn whole, nor a
/// reference part, which shows whole around the cut.
#[test]
fn mesh_bodies_and_reference_parts_are_not_hatched() {
    let mut s = block();
    with_mesh_cube(&mut s, [60.0, 10.0, 30.0], "hatch");
    let (s, cut) = front_section(s, &[[50.0, -10.0], [50.0, 70.0]]);
    let (set, hatches) = drawn_hatches(&s, cut);
    let names: Vec<&str> = hatches.iter().map(|h| set.bodies[h.shown].body.as_str()).collect();
    assert_eq!(names, ["Block"]);
    let (s, _) = plate_and_block(5.0);
    let (s, cut) = front_section(s, &[[35.0, -10.0], [35.0, 30.0]]);
    let (set, hatches) = drawn_hatches(&s, cut);
    let names: Vec<&str> = hatches.iter().map(|h| set.bodies[h.shown].body.as_str()).collect();
    assert_eq!(names, ["Plate"]);
}

/// Two blocks side by side, cut along the face where they meet, and a bar across the plane beside
/// them. The block on the kept side is shown whole: its face on the plane faces out of the kept
/// side, but no material was cut there, so it isn't cut or hatched. The bar is cut and hatched.
#[test]
fn a_cut_along_a_mating_face_hatches_no_uncut_face() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [20, 0, 0], "operation": "new", "body_name": "B"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 20, "corner": [10, 40, 0], "operation": "new", "body_name": "Bar"}));
    let (s, cut) = front_section(s, &[[20.0, -10.0], [20.0, 30.0]]);
    let (set, hatches) = drawn_hatches(&s, cut);
    // The case is reached: the block on the kept side is shown, and no cut failed.
    let shown: Vec<&str> = set.bodies.iter().map(|b| b.body.as_str()).collect();
    assert!(set.uncut.is_none() && shown.iter().any(|n| matches!(*n, "A" | "B")), "{shown:?} {:?}", set.uncut);
    let cut_bodies: Vec<&str> = set.bodies.iter().filter(|b| set.cut.contains_key(&b.index)).map(|b| b.body.as_str()).collect();
    assert_eq!(cut_bodies, ["Bar"]);
    let names: Vec<&str> = hatches.iter().map(|h| set.bodies[h.shown].body.as_str()).collect();
    assert_eq!(names, ["Bar"]);
}

/// `s`'s bodies written to STEP and read back as the one feature of a new design, as an imported
/// part arrives: no modelling history of its own.
fn imported(s: &Session) -> Session {
    let step = solidcraft_io::export(&s.model.state(), &[], solidcraft_io::Format::Step, "Parts").unwrap();
    let imported = solidcraft_io::step_import_feature(&step, "parts.step").unwrap();
    let mut doc = solidcraft_doc::Document::new("Imported parts");
    let import = doc.add_feature(imported.kind, None).unwrap();
    doc.feature_mut(import).unwrap().body_names = imported.body_names;
    Session::new(doc)
}

/// A 20 mm block imported from STEP and cut through its middle: its cut face is hatched, and the
/// view has no doubt to report (nothing of the block lay on the plane before the cut).
#[test]
fn an_imported_part_cut_through_its_middle_is_hatched() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "Block"}));
    let (s, cut) = front_section(imported(&s), &[[10.0, -10.0], [10.0, 30.0]]);
    let (set, hatches) = drawn_hatches(&s, cut);
    assert_eq!(set.cut.len(), 1, "{:?}", set.cut);
    assert!(hatches.len() == 1 && !hatches[0].filled && !hatches[0].strokes.is_empty(), "{hatches:?}");
}

/// The two blocks of [`a_cut_along_a_mating_face_hatches_no_uncut_face`], imported from STEP and cut
/// along the face where they meet: the block on the kept side only touches the plane, so nothing of
/// it is cut or hatched, whatever history the cut kept.
#[test]
fn an_imported_part_touching_the_plane_is_not_hatched() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [20, 0, 0], "operation": "new", "body_name": "B"}));
    let (s, cut) = front_section(imported(&s), &[[20.0, -10.0], [20.0, 30.0]]);
    let (set, hatches) = drawn_hatches(&s, cut);
    assert!(set.uncut.is_none() && !set.bodies.is_empty(), "{:?}", set.uncut);
    assert!(set.cut.is_empty() && hatches.is_empty(), "{:?} {hatches:?}", set.cut);
}

/// A section view's sheet draws its hatching under its lines, and `drawing.scene` lists it per body:
/// the angle, the spacing, whether it is filled and how many strokes it drew, as the view's own
/// hatching says.
#[test]
fn a_sheet_draws_a_sections_hatching_and_says_so() {
    let (mut s, _, cut) = drilled_with_tab();
    let (set, hatches) = drawn_hatches(&s, cut);
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let scene = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<crate::schema::drawing_scene::SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &scene), "{scene}");
    let state = scene["views"].as_array().unwrap().iter().find(|v| v["view"] == json!(ids::encode(cut))).unwrap().clone();
    let want: Vec<Value> = hatches
        .iter()
        .map(|h| {
            json!({
                "body": set.bodies[h.shown].body,
                "angle_deg": h.angle.to_degrees(),
                "spacing_mm": h.spacing_mm,
                "filled": h.filled,
                "strokes": h.strokes.len(),
            })
        })
        .collect();
    assert_eq!(state["hatches"], json!(want), "{state}");
    assert!(want.iter().all(|h| h["strokes"].as_u64().unwrap() > 0), "{want:?}");
}

/// A flange `scale` times the size of a 100 mm one: a 30 mm bore and eight 10 mm holes on a 70 mm
/// circle, 10 mm thick.
fn flange(scale: f64) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"diameter": 100.0 * scale, "height": 10.0 * scale, "body_name": "Flange"}));
    let bore = json!({"diameter": 30.0 * scale, "height": 12.0 * scale, "base": [0.0, 0.0, -scale], "operation": "cut", "targets": ["Flange"]});
    run(&mut s, "solid.cylinder", bore);
    for k in 0..8 {
        let a = f64::from(k) * std::f64::consts::TAU / 8.0;
        let base = [35.0 * scale * a.cos(), 35.0 * scale * a.sin(), -scale];
        run(
            &mut s,
            "solid.cylinder",
            json!({"diameter": 10.0 * scale, "height": 12.0 * scale, "base": base, "operation": "cut", "targets": ["Flange"]}),
        );
    }
    s
}

/// The flange cut across its holes, at `scale` on paper.
fn flange_section(size: f64, scale: &str) -> (Session, u64) {
    let (mut s, cut) = front_section(flange(size), &[[-60.0 * size, 5.0 * size], [60.0 * size, 5.0 * size]]);
    edit_view(&mut s, cut, json!({"scale": scale}));
    (s, cut)
}

/// An 800 mm flange cut across at 1:1, larger than the biggest section an A1 sheet holds at 1:1, is
/// hatched: its outlines are followed closely enough for the drawing's hatcher, within its budget.
#[test]
fn a_large_flange_at_full_size_is_hatched_within_the_hatchers_budget() {
    let (s, cut) = flange_section(8.0, "1:1");
    let (set, hatches) = drawn_hatches(&s, cut);
    let flange = hatch_of(&set, &hatches, "Flange");
    assert!(!flange.filled && flange.strokes.len() > 1000, "{} strokes", flange.strokes.len());
}

/// Every hatch line of the 100 mm flange ends on its cut face's true outline, to within the
/// flattening the hatcher is given: under the outline's own line.
#[test]
fn hatch_lines_end_on_the_true_outline_within_the_flattening() {
    use crate::drawing_view::FLATTEN_MM;
    let (s, cut) = flange_section(1.0, "1:1");
    let (set, hatches) = drawn_hatches(&s, cut);
    let flange = hatch_of(&set, &hatches, "Flange");
    assert_hatching_inside(flange);
    assert!(flange.strokes.len() > 100, "{} strokes", flange.strokes.len());
    for stroke in &flange.strokes {
        let (a, b) = stroke_ends(stroke);
        for p in [a, b] {
            let off = off_outline(p, &flange.regions);
            assert!(off <= FLATTEN_MM + 1e-9, "a hatch line ends {off} mm off the outline at {p:?}");
        }
    }
}

/// #20733: a section the mesh projection draws (a modelled thread) shows its cut body: the bore's
/// cut edges are drawn visible.
#[test]
fn a_section_drawn_from_the_mesh_draws_the_cut_body() {
    use solidcraft_doc::drawing::cutting_line::{ArrowSide, SectionKind};
    let mut s = block();
    run(&mut s, "solid.cylinder", json!({"diameter": 8.5, "height": 22, "base": [50, 25, -1], "operation": "cut", "targets": ["Block"]}));
    run(&mut s, "solid.thread", json!({"face": [54.25, 25, 10], "designation": "M10", "modeled": true}));
    let (mut s, views) = three_views_of(s);
    let cut = section_of(&mut s, views[0], SectionKind::Full, &[[50.0, -10.0], [50.0, 70.0]], ArrowSide::Right);
    let (_, _, lines) = drawn_lines(&s, cut);
    assert_eq!(lines.made.exact, 0, "{:?}", lines.made);
    let cut_edges = |side: f64| {
        let pieces = lines.lines.iter().flat_map(|l| &l.pieces).filter(|p| p.visibility == Visibility::Visible);
        pieces
            .filter(|p| {
                p.path.iter().all(|seg| match seg {
                    solidcore_record::Seg2::Line { a, b } => [a.x, b.x].iter().all(|x| (2.0..=2.6).contains(&(x * side))),
                    _ => false,
                })
            })
            .count()
    };
    assert!(cut_edges(1.0) > 0 && cut_edges(-1.0) > 0, "{} {}", cut_edges(1.0), cut_edges(-1.0));
}

/// The body names `v`'s lines name, each once.
fn line_bodies(v: &Value) -> Vec<Value> {
    let mut names: Vec<Value> = v["lines"].as_array().unwrap().iter().map(|l| l["body"].clone()).collect();
    names.sort_by_key(|n| n.to_string());
    names.dedup();
    names
}

/// `hide_bodies` leaves out one body of a part (#22107): from the front at 1:1 the carcass and the
/// door are 220 mm across, the carcass alone 100. A body no part has, or none at that path, is
/// refused by name; hiding every body leaves nothing and is refused; an empty name is refused.
#[test]
fn a_snapshots_hide_bodies_leaves_out_one_body() {
    let mut s = carcass_and_door();
    let both = snap(&mut s, json!({"scale": "1:1"}));
    assert_eq!(line_bodies(&both), [json!("Carcass"), json!("Door")]);
    assert!((boundary(&both).0.x - 220.0).abs() < 1e-6, "{both}");
    let one = snap(&mut s, json!({"scale": "1:1", "hide_bodies": [{"path": [], "body": "Door"}]}));
    assert_eq!(line_bodies(&one), [json!("Carcass")]);
    assert!((boundary(&one).0.x - 100.0).abs() < 1e-6 && one["source"]["bodies"] == 1, "{one}");
    let e = refused(&mut s, json!({"hide_bodies": [{"body": "Lid"}]}));
    assert!(e.contains("hide_bodies: no body `Lid` among the design's own bodies"), "{e}");
    let e = refused(&mut s, json!({"hide_bodies": [{"path": [7], "body": "Door"}]}));
    assert!(e.contains("hide_bodies: no body `Door` at occurrence path [7]"), "{e}");
    let e = refused(&mut s, json!({"hide_bodies": [{"body": "Door"}, {"body": "Carcass"}]}));
    assert!(e.contains("the source and hide leave no bodies to draw"), "{e}");
    for (p, why) in [
        (json!({"hide_bodies": [{"body": ""}]}), "named in 1 to 128 characters"),
        (json!({"hide_bodies": [{"body": "D".repeat(129)}]}), "named in 1 to 128 characters"),
        (json!({"hide_bodies": [{"body": "Door", "side": 1}]}), "side"),
        (json!({"hide_bodies": [{"path": []}]}), "body"),
    ] {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(why), "{p}: {e}");
    }
}

/// `hide_bodies` on a saved view (#22107): made with it, the base view fits the carcass alone;
/// inspect says where it comes from and lists every body with whether it draws it; a projected view follows it, naming its parent, until it sets its
/// own (`[]` hides nothing); `null` follows again; an edit is one undo step; a copy alone keeps it. A
/// body the design doesn't place is refused by name and changes nothing.
#[test]
fn a_saved_views_hide_bodies_follows_its_chain() {
    let mut s = carcass_and_door();
    run(&mut s, "drawing.create", json!({}));
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let door = json!([{"path": [], "body": "Door"}]);
    let (_, view) = create(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "position_mm": [200, 150], "hide_bodies": door}));
    assert!((width(&mut s, view) - 100.0).abs() < 1e-6);
    let inspected = inspect_view(&mut s, view);
    assert_eq!(inspected["settings"]["hide_bodies"], json!({"value": door, "from": "this", "set": door}), "{inspected}");
    assert!(inspected.get("unmatched").is_none(), "{inspected}");
    // Every body the design places, before what the view leaves out, and whether it draws it.
    let every_body = json!([{"path": [], "body": "Carcass", "drawn": true}, {"path": [], "body": "Door", "drawn": false}]);
    assert_eq!(inspected["bodies"], every_body, "{inspected}");

    let (_, side) = create(&mut s, PROJECTED, json!({"parent": ids::encode(view), "side": "above", "place": {"at": [200, 250]}}));
    assert!((width(&mut s, side) - 100.0).abs() < 1e-6, "it follows its parent");
    let inspected = inspect_view(&mut s, side);
    assert_eq!(
        inspected["settings"]["hide_bodies"],
        json!({"value": door, "from": "parent", "parent": ids::encode(view), "set": null}),
        "{inspected}"
    );
    let steps = s.undo.len();
    let reply = edit_view(&mut s, side, json!({"hide_bodies": []}));
    assert_eq!((reply["changed"].clone(), s.undo.len()), (json!(["hide_bodies"]), steps + 1));
    assert!((width(&mut s, side) - 220.0).abs() < 1e-6, "an empty list hides nothing, whatever the parent hides");
    edit_view(&mut s, side, json!({"hide_bodies": null}));
    assert!((width(&mut s, side) - 100.0).abs() < 1e-6, "null follows the parent again");
    run(&mut s, "edit.undo", json!({}));
    assert!((width(&mut s, side) - 220.0).abs() < 1e-6);

    let alone = listed(&to_sheet(&mut s, COPY, json!({"view": ids::encode(view), "sheet": "Sheet 1", "position_mm": [300, 100]})))[0];
    assert_eq!(saved(&s, alone).settings.hide_bodies.as_deref(), Some(&[HiddenBody { path: vec![], body: "Door".into() }][..]));

    let before = s.drawing("test").unwrap().to_json().unwrap();
    let e = edit_refused(&mut s, view, json!({"hide_bodies": [{"body": "Lid"}]}));
    assert!(e.contains("hide_bodies: no body `Lid` among the design's own bodies"), "{e}");
    let e = edit_refused(&mut s, view, json!({"hide_bodies": [{"body": "Door"}, {"body": "Carcass"}]}));
    assert!(e.contains("the source and hide leave no bodies to draw"), "{e}");
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
    let e = create_refused(&mut s, BASE, json!({"sheet": sheet, "hide_bodies": [{"path": [9], "body": "Door"}]}));
    assert!(e.contains("hide_bodies: no body `Door` at occurrence path [9]"), "{e}");
    edit_view(&mut s, view, json!({"hide_bodies": null}));
    assert!((width(&mut s, view) - 220.0).abs() < 1e-6, "null clears it");
}

/// A hidden body renamed since matches nothing (#22107, as dimensions don't follow a rename, #4892):
/// the view draws it again, and inspect lists the entry as `unmatched` until the old name is back.
#[test]
fn a_renamed_hidden_body_is_unmatched() {
    let mut s = carcass_and_door();
    run(&mut s, "drawing.create", json!({}));
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let (_, view) = create(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "hide_bodies": [{"body": "Door"}]}));
    assert!((width(&mut s, view) - 100.0).abs() < 1e-6);
    let drawing = ids::encode(s.drawing("test").unwrap().id);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "body.rename", json!({"body": "Door", "name": "Gate"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let inspected = inspect_view(&mut s, view);
    assert_eq!(inspected["unmatched"], json!([{"path": [], "body": "Door"}]), "{inspected}");
    let gate = json!([{"path": [], "body": "Carcass", "drawn": true}, {"path": [], "body": "Gate", "drawn": true}]);
    assert_eq!(inspected["bodies"], gate, "the renamed body is listed by its new name, drawn: {inspected}");
    assert!((width(&mut s, view) - 220.0).abs() < 1e-6, "the renamed body is drawn again");
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "body.rename", json!({"body": "Gate", "name": "Door"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert!(inspect_view(&mut s, view).get("unmatched").is_none());
    assert!((width(&mut s, view) - 100.0).abs() < 1e-6);
}

/// `shows: "design"` (#22107): a view draws only the bodies the browser shows. Hiding the door's eye
/// takes it out of that view and of the view projected from it, never of a view of every body.
/// Hiding the Bodies folder leaves its placeholder (the design is never refused by its drawings);
/// isolating the carcass leaves the carcass. Inspect says where the setting comes from, and `"all"`
/// draws every body again. A value other than `all` or `design` is refused.
#[test]
fn a_view_that_shows_the_design_follows_the_browsers_eyes() {
    let mut s = carcass_and_door();
    run(&mut s, "drawing.create", json!({}));
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let (_, view) = create(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "position_mm": [200, 150], "shows": "design"}));
    let (_, every) = create(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "position_mm": [200, 60]}));
    assert!((width(&mut s, view) - 220.0).abs() < 1e-6, "nothing hidden yet");
    let inspected = inspect_view(&mut s, view);
    assert_eq!(inspected["settings"]["shows"], json!({"value": "design", "from": "this", "set": "design"}), "{inspected}");
    assert_eq!(inspect_view(&mut s, every)["settings"]["shows"], json!({"value": "all", "from": "default", "set": null}));

    run(&mut s, "browser.visibility", json!({"items": ["Door"], "visible": false}));
    assert!((width(&mut s, view) - 100.0).abs() < 1e-6, "the door is hidden in the browser");
    assert!((width(&mut s, every) - 220.0).abs() < 1e-6, "a view of every body still draws it");
    // Inspect still lists every body the design places, and says the hidden door isn't drawn.
    let listed = |drawn_door: bool| json!([{"path": [], "body": "Carcass", "drawn": true}, {"path": [], "body": "Door", "drawn": drawn_door}]);
    assert_eq!(inspect_view(&mut s, view)["bodies"], listed(false));
    assert_eq!(inspect_view(&mut s, every)["bodies"], listed(true), "a view of every body draws the door");
    let (_, side) = create(&mut s, PROJECTED, json!({"parent": ids::encode(view), "side": "above", "place": {"at": [200, 250]}}));
    assert!((width(&mut s, side) - 100.0).abs() < 1e-6, "a projected view follows its parent's choice");

    run(&mut s, "browser.visibility", json!({"tree_folders": ["c0/bodies"], "visible": false}));
    let extents = inspect_view(&mut s, view)["extents"].clone();
    assert_eq!((&extents["status"], &extents["provenance"]), (&json!("placeholder"), &json!("nothing_chosen")), "{extents}");
    run(&mut s, "browser.visibility", json!({"tree_folders": ["c0/bodies"], "visible": true}));
    assert!((width(&mut s, view) - 100.0).abs() < 1e-6, "the door keeps its own eye");

    run(&mut s, "browser.visibility", json!({"items": ["Door"], "visible": true}));
    run(&mut s, "browser.visibility", json!({"items": ["Carcass"], "visible": true, "isolate": true}));
    assert!((width(&mut s, view) - 100.0).abs() < 1e-6, "isolation is what the design shows");
    run(&mut s, "browser.visibility", json!({"visible": true, "unisolate": true}));
    assert!((width(&mut s, view) - 220.0).abs() < 1e-6);

    run(&mut s, "browser.visibility", json!({"items": ["Door"], "visible": false}));
    let reply = edit_view(&mut s, view, json!({"shows": "all"}));
    assert_eq!(reply["changed"], json!(["shows"]));
    assert!((width(&mut s, view) - 220.0).abs() < 1e-6, "every body again");
    for bad in [json!("Design"), json!(5), json!("visible")] {
        let e = edit_refused(&mut s, view, json!({"shows": bad}));
        assert!(e.contains("shows"), "{bad}: {e}");
    }
}

/// `shows: "design"` follows the eyes of the design the drawing is in; a view of a linked design's
/// file can't read that design's eyes yet (#3890), so it is refused by name.
#[test]
fn a_view_of_a_linked_design_cant_follow_its_eyes() {
    let mut s = carcass_and_door();
    run(&mut s, "drawing.create", json!({}));
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let (_, view) = create(&mut s, BASE, json!({"sheet": sheet, "scale": "1:1", "position_mm": [200, 150]}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    let linked = d.sources.iter().map(|source| source.id).max().unwrap() + 1;
    d.sources.push(solidcraft_doc::drawing::Source { id: linked, path: Some("plate.solidcraft".into()) });
    let mut record = View::clone(d.view(view).unwrap());
    if let ViewKind::Base { source, .. } = &mut record.kind {
        *source = linked;
    }
    d.views.retain(|v| v.id != view);
    d.add_views([Arc::new(record)]);
    s.doc_mut().put_drawing(Arc::new(d));
    let e = edit_refused(&mut s, view, json!({"shows": "design"}));
    assert!(e.contains("not supported yet: following the eyes of a linked design"), "{e}");
}
