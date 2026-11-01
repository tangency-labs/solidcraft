use std::sync::Arc;

use serde_json::json;
use solidcraft_doc::MAX_PLACEMENT_DEPTH;
use solidcraft_doc::drawing::DEFAULT_DPI;
use solidcraft_geom::cell::Projection;
use solidcraft_geom::drawing_view::ProjectedSide;
use solidcraft_geom::paper::NoPictures;

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
        (json!({"tangent_edges": "thin"}), "thin"),
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
/// covered; the edges seen end on draw nothing. The lines are the mesh's, so they say sampled.
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
        assert!(line["pieces"].as_array().unwrap().iter().all(|p| p["accuracy"]["kind"] == "sampled"), "{line}");
    }
    assert_eq!((&v["status"]["lines"], &v["status"]["provenance"]), (&json!("approximate"), &json!("mesh_projection")));
    assert!(v["status"]["covered_pieces"].as_u64().unwrap() >= 4, "{v}");
}

/// A 20 x 20 x 40 tower behind the block, 40 to 60 mm across: in the front view at 1:1 its sides
/// show above the block (20 to 40 mm up) and are hidden behind it below, 2 x 20 mm of hidden line,
/// which only `visible_hidden` draws.
#[test]
fn hidden_lines_are_drawn_only_when_the_style_asks() {
    let mut s = block();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 40, "corner": [40, 60, 0], "body_name": "Tower", "operation": "new"}));
    let v = snap(&mut s, json!({"scale": "1:1", "display_style": "visible_hidden"}));
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
    let dxf = s.execute("file.export_view_dxf", &json!({"camera": solidcraft_render::Camera::default(), "tolerance": 2})).unwrap_err().to_string();
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
    let v = snap(&mut s, json!({"scale": "1:1", "display_style": "visible_hidden"}));
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
fn drawn(v: &Value, visibility: &str) -> Vec<solidcraft_geom::Seg2> {
    let pieces = v["lines"].as_array().into_iter().flatten().flat_map(|l| l["pieces"].as_array().into_iter().flatten());
    let segs = pieces.filter(|p| p["visibility"] == visibility).flat_map(|p| p["path"].as_array().into_iter().flatten());
    segs.map(|seg| serde_json::from_value(seg.clone()).unwrap()).collect()
}

/// The points of `seg` furthest out: its ends, and for an arc each quarter-turn point it passes.
fn reach(seg: &solidcraft_geom::Seg2) -> Vec<Vec2> {
    let mut out = vec![seg.start(), seg.end()];
    if let solidcraft_geom::Seg2::Arc { center, radius, start, sweep } = *seg {
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
fn share_a_stretch(a: &solidcraft_geom::Seg2, b: &solidcraft_geom::Seg2) -> bool {
    use solidcraft_geom::Seg2::{Arc, Line};
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
            "display_style": style,
            "scene": style == "visible",
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
        if style == "visible" {
            let scene: solidcraft_geom::paper::Scene = serde_json::from_value(v["scene"].clone()).unwrap();
            assert!(scene.validate().is_ok(), "{p}");
            let pieces = v["lines"].as_array().unwrap().iter().flat_map(|l| l["pieces"].as_array().unwrap()).count();
            assert_eq!(strokes(&v).len(), pieces, "{p}: a stroke per piece");
        }
    }
}

/// `chord_mm` and `max_error_mm` out of range are refused by name; a `max_error_mm` the mesh's
/// lines can't certify is refused with the count of pieces short of it.
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
        (json!({"scale": "1:2", "max_error_mm": 0.5}), "accuracy not met: 0 bounded and"),
    ] {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(says), "{p}: {e}");
        assert!(std::sync::Arc::ptr_eq(&doc, &s.doc), "{p}");
    }
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
/// visible. The cut arcs end where the mesh found the change, so they stay sampled.
#[test]
fn a_rim_cut_where_its_visibility_changes_stays_sampled() {
    let mut s = plate();
    run(&mut s, "solid.box", json!({"length": 30, "width": 4, "height": 2, "corner": [5, 18, 10], "body_name": "Bar", "operation": "new"}));
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1", "display_style": "visible_hidden"}));
    let hidden = 4.0 * 0.4f64.asin();
    for (visibility, want) in [("hidden", hidden), ("visible", std::f64::consts::TAU - hidden)] {
        let rims = arcs(&v, visibility);
        let on_rim = |(c, r, _, a): &(Vec2, f64, f64, Value)| c.dist(Vec2::new(210.0, 148.5)) < 1e-9 && (r - 5.0).abs() < 1e-9 && a == "sampled";
        assert!(!rims.is_empty() && rims.iter().all(on_rim), "{visibility}: {v}");
        let round: f64 = rims.iter().map(|(_, _, sweep, _)| sweep.abs()).sum();
        assert!((round - want).abs() < 0.02, "{visibility}: {round}, not {want}: {v}");
    }
}

/// A part with a hole placed three times, the third turned a quarter turn: one mesh serves all
/// three, and each hole is an exact circle where its own placement puts it, 50 mm apart on the
/// sheet round its centre.
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
    assert_eq!(v["stats"]["shared_meshes"], json!(1), "{v}");
    let mut round: std::collections::BTreeMap<i64, f64> = std::collections::BTreeMap::new();
    for (centre, radius, sweep, accuracy) in arcs(&v, "visible") {
        assert!((centre.y - 148.5).abs() < 1e-9 && (radius - 5.0).abs() < 1e-9 && accuracy == "analytic", "{v}");
        *round.entry(centre.x.round() as i64).or_default() += sweep.abs();
    }
    assert_eq!(round.keys().copied().collect::<Vec<_>>(), [160, 210, 260], "{v}");
    assert!(round.values().all(|r| (r - std::f64::consts::TAU).abs() < 1e-9), "{v}");
}

/// Every stroke of `v`'s paper scene: its path, whether it is closed, and its width.
fn strokes(v: &Value) -> Vec<(Vec<solidcraft_geom::Seg2>, bool, f64)> {
    use solidcraft_geom::paper::{Item, Scene};
    let scene: Scene = serde_json::from_value(v["scene"].clone()).unwrap();
    let items = scene.groups.into_iter().flat_map(|g| g.items);
    items
        .filter_map(|item| match item {
            Item::Stroke { path, closed, width_mm } => Some((path, closed, width_mm)),
            Item::Fill { .. } | Item::Picture(_) => None,
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

/// The plate's hole in the scene, from above at 1:1: its rim's strokes are arcs of radius 5 round
/// the sheet's centre, 2π × 5 mm in all, and a stroke that is a whole circle is closed.
#[test]
fn a_holes_rim_is_arcs_in_the_scene() {
    let mut s = plate();
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1", "scene": true}));
    let mut round = 0.0;
    for (path, closed, _) in strokes(&v) {
        for seg in &path {
            if let solidcraft_geom::Seg2::Arc { center, radius, sweep, .. } = *seg {
                assert!(center.dist(Vec2::new(210.0, 148.5)) < 1e-9 && (radius - 5.0).abs() < 1e-9, "{v}");
                round += radius * sweep.abs();
                let whole = path.len() == 1 && (sweep.abs() - std::f64::consts::TAU).abs() < 1e-9;
                assert!(closed || !whole, "a whole circle is closed: {v}");
            }
        }
    }
    assert!((round - std::f64::consts::TAU * 5.0).abs() < 1e-6, "{round}: {v}");
}

/// `scene` is true or false, and with `visible_hidden` it waits for dashed lines (V7b).
#[test]
fn a_scene_with_hidden_lines_or_a_bad_flag_is_refused() {
    let mut s = block();
    let doc = s.doc.clone();
    for (p, says) in [
        (json!({"scene": "yes"}), "scene"),
        (json!({"scene": 1}), "scene"),
        (json!({"scene": null}), "scene"),
        (json!({"scene": true, "display_style": "visible_hidden"}), "not supported yet: hidden lines in the paper scene"),
    ] {
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

/// Worked by hand, top view at 1:1 with a margin taking in all of the block. The plate's outline is
/// 160 mm, 20 of it (x = 40, y 10..30) under the block when the block is above; the block's is 80,
/// and when it is below, its parts at x 30..40 are under the plate: 40 hidden and 40 visible.
/// Reference parts cover the model only when told, and the plate always covers the block.
#[test]
fn reference_lines_are_their_own_and_cover_the_model_only_when_told() {
    let view = |hides: bool| {
        json!({"view": "top", "scale": "1:1", "display_style": "visible_hidden", "reference_style": "visible_hidden",
               "reference_hides_model": hides, "reference_margin_mm": 20})
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
/// model's only with `display_style`.
#[test]
fn each_part_of_a_view_draws_hidden_lines_by_its_own_style() {
    let (mut above, _) = plate_and_block(5.0);
    let hides = json!({"view": "top", "scale": "1:1", "reference_hides_model": true, "reference_margin_mm": 20});
    let v = snap(&mut above, hides.clone());
    assert_lengths(&v, false, 140.0, 0.0);
    let (mut below, _) = plate_and_block(-10.0);
    let v = snap(&mut below, hides);
    assert_lengths(&v, true, 40.0, 0.0);
    let v = snap(&mut below, json!({"view": "top", "scale": "1:1", "reference_style": "visible_hidden", "reference_margin_mm": 20}));
    assert_lengths(&v, true, 40.0, 40.0);
}

/// With model parts there, the boundary is theirs and the margin's, and reference lines stop at it:
/// with no margin the block keeps x 30..40 (20 + 10 + 10); with 5 mm, x 30..45 (20 + 15 + 15).
#[test]
fn reference_parts_stop_at_the_boundary_of_the_model_and_its_margin() {
    let (mut s, _) = plate_and_block(5.0);
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1"}));
    assert_box(&v, [190.0, 128.5], [230.0, 168.5]);
    assert_eq!(v["source"]["bodies"], 2, "{v}");
    assert_lengths(&v, true, 40.0, 0.0);
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1", "reference_margin_mm": 5}));
    assert_box(&v, [185.0, 123.5], [235.0, 173.5]);
    assert_lengths(&v, true, 50.0, 0.0);
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
/// apart, each keeps its whole outline, so the model's line is drawn in the model's width.
#[test]
fn a_reference_line_never_takes_a_stretch_from_a_model_line() {
    let (mut s, [_, block]) = plate_and_block(5.0);
    run(&mut s, "occurrence.move", json!({"occurrence": block, "translate": [-10, 0, 0]}));
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

/// The snapshot's scene draws reference parts as saved views do: thin, and first.
#[test]
fn a_scene_draws_reference_parts_thin_under_the_model() {
    let (mut s, _) = plate_and_block(5.0);
    let v = snap(&mut s, json!({"view": "top", "scale": "1:1", "scene": true, "reference_margin_mm": 20}));
    let widths: Vec<f64> = strokes(&v).iter().map(|(_, _, width)| *width).collect();
    let thin = widths.iter().take_while(|w| **w == crate::drawing_view::REFERENCE_MM).count();
    assert!(thin > 0 && widths[thin..].iter().all(|w| *w == crate::drawing_view::VISIBLE_MM), "{widths:?}");
}

/// Hidden reference lines wait for dashed lines on paper as the model's do.
#[test]
fn a_scene_with_hidden_reference_lines_is_refused() {
    let (mut s, _) = plate_and_block(5.0);
    let e = refused(&mut s, json!({"scene": true, "reference_style": "visible_hidden"}));
    assert!(e.contains("not supported yet: hidden lines"), "{e}");
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
