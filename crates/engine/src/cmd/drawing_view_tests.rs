use std::sync::Arc;

use serde_json::json;
use solidcraft_doc::MAX_PLACEMENT_DEPTH;
use solidcraft_geom::cell::Projection;
use solidcraft_geom::drawing_view::ProjectedSide;

use super::*;
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
fn three_views_of(mut s: Session) -> (Session, [u64; 3]) {
    run(&mut s, "drawing.create", json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    let mut id_source = solidcraft_doc::ids::IdSource::seeded(4816);
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    let mut add = |d: &mut Drawing, kind: ViewKind, place: ViewPlace, settings: ViewOverrides| {
        let id = d.allocate_id(&mut id_source).unwrap();
        d.add_views([Arc::new(View { id, sheet, kind, place, settings, reference_margin_mm: 0.0 })]);
        id
    };
    let front_settings =
        ViewOverrides { scale: Some("1:2".parse().unwrap()), display_style: Some(VisualStyle::VisibleHidden), ..ViewOverrides::default() };
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

/// Part `Part` with a 10 mm cube `Cube` from the origin, placed twice: the second copy 30 mm along
/// X. The session and the two occurrences.
fn two_cubes() -> (Session, [u64; 2]) {
    let mut s = Session::default();
    let created = run(&mut s, "component.create", json!({"name": "Part"}));
    let first = created["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube", "operation": "new"}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [30, 0, 0]}))["occurrence"].as_u64().unwrap();
    (s, [first, second])
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
    assert_eq!(base["settings"]["display_style"], json!({"value": "visible_hidden", "from": "this", "set": "visible_hidden"}));
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
    assert_eq!(chained["settings"]["display_style"], json!({"value": "visible_hidden", "from": "parent", "parent": front_id, "set": null}));
    assert!(chained["direction"].get("named").is_none(), "the right view's top view is no named view: {chained}");
    assert_eq!(chained["choices"]["display_style"].as_array().unwrap().len(), 5);
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
    let edited = edit_view(&mut s, right, json!({"display_style": "visible"}));
    assert_eq!((edited["changed"].clone(), edited["followers"].clone()), (json!(["display_style"]), json!([ids::encode(top)])));
    assert_eq!(s.undo.len(), steps + 1);
    assert_eq!(inspect_view(&mut s, right)["linked"], "partly");
    assert_eq!(inspect_view(&mut s, top)["settings"]["display_style"]["parent"], json!(ids::encode(right)));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(inspect_view(&mut s, right)["settings"]["display_style"]["from"], "parent");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(edit_view(&mut s, right, json!({"display_style": null}))["changed"], json!(["display_style"]));
    assert_eq!(inspect_view(&mut s, right)["settings"]["display_style"]["from"], "parent", "null follows the parent again");
    // Unlinked, the display settings are its own at the values they had; linked, they follow again.
    edit_view(&mut s, right, json!({"linked": false}));
    let unlinked = inspect_view(&mut s, right);
    assert_eq!((unlinked["linked"].clone(), unlinked["settings"]["display_style"]["set"].clone()), (json!(false), json!("visible_hidden")));
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
    assert_eq!(edit_view(&mut s, right, json!({"place": {"at": [320.0, 150.0]}}))["changed"], json!(["place"]));
    assert_eq!(edit_view(&mut s, top, json!({"reference_margin_mm": 5.0}))["changed"], json!(["reference_margin_mm"]));
    assert_eq!(inspect_view(&mut s, top)["reference_margin_mm"], 5.0);
    assert!(edit_refused(&mut s, top, json!({"reference_margin_mm": -1.0})).contains("reference margin"));
    // Nothing changed, no step.
    let steps = s.undo.len();
    assert_eq!(edit_view(&mut s, front, json!({"scale": "1:5"}))["changed"], json!([]));
    assert_eq!(s.undo.len(), steps, "no change, no step");
    assert_eq!(s.evaluations, evaluations, "no model is evaluated");
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
        display_style: Some(VisualStyle::VisibleHidden),
        reference_style: Some(VisualStyle::Visible),
        reference_hides_model: Some(false),
        tangent_edges: Some(Default::default()),
    };
    assert_eq!(made.settings, own);
    let of_top = copy(&mut s, top);
    let d = s.drawing("test").unwrap().clone();
    let ViewKind::Base { view: ViewDirection::Looking(_), .. } = saved(&s, of_top).kind else { panic!("the right view's top view is no named view") };
    let (was, is) = (d.resolve_view(top).unwrap().basis, d.resolve_view(of_top).unwrap().basis);
    for (a, b) in [(was.x, is.x), (was.y, is.y), (was.origin, is.origin)] {
        assert!((a - b).len() < 1e-12, "{a:?} {b:?}");
    }
    edit_view(&mut s, front, json!({"display_style": "visible", "scale": "1:10"}));
    assert_eq!(saved(&s, of_right).settings, own, "the copy follows nothing");
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

/// Scale and position only scale and move the boundary; the margin grows each side by the
/// margin times the scale; the browser's eye counts for nothing; nothing in the design changes.
#[test]
fn scale_position_and_margin_set_the_boundary_and_the_design_is_untouched() {
    let mut s = block();
    let (undo, doc) = (s.undo.len(), s.doc.clone());
    let dirty = s.is_dirty();
    let one = boundary(&snap(&mut s, json!({"scale": "1:1", "position_mm": [0, 0]})));
    let half = boundary(&snap(&mut s, json!({"scale": "1:2", "position_mm": [0, 0]})));
    assert!((one.0 - half.0 * 2.0).len() < EPS && one.1.len() < EPS && half.1.len() < EPS);
    let margin = boundary(&snap(&mut s, json!({"scale": "1:2", "position_mm": [0, 0], "reference_margin_mm": 5})));
    assert!((margin.0 - (half.0 + Vec2::new(5.0, 5.0))).len() < EPS, "2.5 mm a side at 1:2: {margin:?}");
    let first = snap(&mut s, json!({"view": "top"}));
    assert_eq!(snap(&mut s, json!({"view": "top"})), first, "the same call gives the same answer");
    assert_eq!((s.undo.len(), s.is_dirty()), (undo, dirty));
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc), "nothing in the design changes");
    run(&mut s, "browser.visibility", json!({"items": ["Block"], "visible": false}));
    assert_eq!(snap(&mut s, json!({"view": "top"}))["boundary_mm"], first["boundary_mm"], "a hidden body is still drawn");
}

/// What the snapshot refuses, each with nothing changed: bad settings, the shaded styles until
/// the drawing renderer, the sheet's resolution out of range, an empty design, a position off
/// the sheet's limit, and a view that would reach past the paper's coordinates.
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
        (json!({"display_style": "shaded"}), "not supported yet"),
        (json!({"reference_style": "shaded_visible"}), "not supported yet"),
        (json!({"display_style": "Shaded"}), "unknown variant"),
        (json!({"sheet": {"dpi": 24.9}}), "dpi"),
        (json!({"sheet": {"dpi": 2400.1}}), "dpi"),
        (json!({"sheet": {"size": {"custom": {"sides_mm": [0, 10]}}}}), "sheet"),
        (json!({"reference_margin_mm": -1}), "reference_margin_mm"),
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
/// view projected from it takes that scale too. The top view's 5 mm reference margin grows it by
/// 2.5 mm a side at 1:2, its extents unchanged. The largest margin at the largest scale still
/// gives a finite box.
#[test]
fn a_lined_up_view_follows_its_parent_and_a_margin_grows_the_boundary() {
    let (mut s, [front, right, top]) = three_views();
    edit_view(&mut s, front, json!({"place": {"at": [160.0, 140.0]}}));
    assert_box(&inspect_view(&mut s, front), [135.0, 135.0], [185.0, 145.0]);
    assert_box(&inspect_view(&mut s, right), [287.5, 135.0], [312.5, 145.0]);
    edit_view(&mut s, top, json!({"reference_margin_mm": 5.0}));
    let margin = inspect_view(&mut s, top);
    assert_box(&margin, [285.0, 222.5], [315.0, 277.5]);
    let size = point(&margin["extents"]["max"]) - point(&margin["extents"]["min"]);
    assert!((size - Vec2::new(50.0, 100.0)).len() < EPS, "{margin}");
    edit_view(&mut s, right, json!({"scale": "1:1"}));
    assert_box(&inspect_view(&mut s, right), [275.0, 130.0], [325.0, 150.0]);
    assert_box(&inspect_view(&mut s, top), [270.0, 195.0], [330.0, 305.0]);
    edit_view(&mut s, top, json!({"reference_margin_mm": 100000.0, "scale": "50:1"}));
    let (huge, centre) = boundary(&inspect_view(&mut s, top));
    assert!(huge.x.is_finite() && huge.x > 1e7 && (centre - Vec2::new(300.0, 250.0)).len() < 1e-6, "{huge:?} {centre:?}");
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
