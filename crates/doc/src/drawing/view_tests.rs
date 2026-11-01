use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_geom::Vec3;
use solidcraft_geom::cell::Projection;
use solidcraft_geom::drawing_sheet::{self, Orientation, PaperSize};
use solidcraft_geom::drawing_view::NamedView;

use super::*;
use crate::drawing::{Excluded, Sheet, Source};
use crate::format::DRAWING;
use crate::ids::IdSource;

const SEED: u64 = 4816;

/// A drawing of one design on one A3 sheet, its front view at 1:2 centred at (150, 150), and the
/// id source that made it.
fn front() -> (Drawing, IdSource, u64) {
    let mut id_source = IdSource::seeded(SEED);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Plate", "en", "Sheet 1", a3, None, &BTreeSet::new(), &mut id_source).unwrap();
    let source = d.allocate_id(&mut id_source).unwrap();
    d.sources.push(Source { id: source });
    let front = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
    let id = add(&mut d, &mut id_source, front, ViewPlace::At([150.0, 150.0]), scaled("1:2"));
    (d, id_source, id)
}

/// A view on the drawing's first sheet, with no reference margin.
fn add(d: &mut Drawing, id_source: &mut IdSource, kind: ViewKind, place: ViewPlace, settings: ViewOverrides) -> u64 {
    let id = d.allocate_id(id_source).unwrap();
    let sheet = d.sheets[0].id;
    d.add_views([Arc::new(View { id, sheet, kind, place, settings, reference_margin_mm: 0.0 })]);
    id
}

/// View `id` of `d`, to change.
fn view_mut(d: &mut Drawing, id: u64) -> &mut View {
    d.views.iter_mut().find(|v| v.id == id).map(Arc::make_mut).unwrap()
}

fn projected(parent: u64, side: ProjectedSide) -> ViewKind {
    ViewKind::Projected { parent, side }
}

fn scaled(scale: &str) -> ViewOverrides {
    ViewOverrides { scale: Some(scale.parse().unwrap()), ..ViewOverrides::default() }
}

fn styled(style: VisualStyle) -> ViewOverrides {
    ViewOverrides { display_style: Some(style), ..ViewOverrides::default() }
}

/// The front view, its right view lined up with it and drawing hidden lines too, and the right
/// view's top view placed apart.
fn three() -> (Drawing, IdSource, [u64; 3]) {
    let (mut d, mut id_source, front) = front();
    let hidden = styled(VisualStyle::VisibleHidden);
    let right = add(&mut d, &mut id_source, projected(front, ProjectedSide::Right), ViewPlace::Aligned(320.0), hidden);
    let top = add(&mut d, &mut id_source, projected(right, ProjectedSide::Above), ViewPlace::At([320.0, 250.0]), ViewOverrides::default());
    d.validate().unwrap();
    (d, id_source, [front, right, top])
}

fn file(d: &Drawing) -> Value {
    serde_json::from_str(&d.to_json().unwrap()).unwrap()
}

/// Why `d`'s file, changed by `edit`, is refused, or "read".
fn refused(d: &Drawing, edit: impl FnOnce(&mut Value)) -> String {
    let mut v = file(d);
    edit(&mut v);
    Drawing::from_json(&v.to_string()).map_or_else(|e| e.to_string(), |_| "read".into())
}

/// A view saves its kind, its place and what it sets itself, and nothing it leaves out; the views
/// save by id, and the drawing saves its projection. It reads back as saved, the same bytes again,
/// and an older drawing reads with no views, in third angle.
#[test]
fn views_save_what_they_set_and_read_back_as_saved() {
    let (d, _, [front, right, top]) = three();
    let v = file(&d);
    let [front_id, right_id, top_id, sheet, source] = [front, right, top, d.sheets[0].id, d.sources[0].id].map(ids::encode);
    assert_eq!(v["projection"], json!("third"));
    assert_eq!(
        v["views"],
        json!([
            {"id": front_id, "sheet": sheet, "kind": {"base": {"source": source, "view": "front"}}, "place": {"at": [150.0, 150.0]}, "settings": {"scale": "1:2"}},
            {"id": top_id, "sheet": sheet, "kind": {"projected": {"parent": right_id, "side": "above"}}, "place": {"at": [320.0, 250.0]}},
            {"id": right_id, "sheet": sheet, "kind": {"projected": {"parent": front_id, "side": "right"}}, "place": {"aligned": 320.0}, "settings": {"display_style": "visible_hidden"}},
        ])
    );
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), d.to_json().unwrap());
    let mut first = v.clone();
    first["projection"] = json!("first");
    assert_eq!(Drawing::from_json(&first.to_string()).unwrap().projection, Projection::First);
    let older = json!({
        "format": DRAWING.format_string(1), "id": "0000000000002", "name": "Old", "sources": [], "language": "en",
        "sheets": [{"id": "0000000000001", "name": "S", "sheet": {"size": {"standard": "a4"}}}]
    });
    let old = Drawing::from_json(&older.to_string()).unwrap();
    assert!(old.views.is_empty() && old.projection == Projection::Third);
    assert_eq!(file(&old).get("views"), None, "no views, none written");
}

/// A drawing lists its views by id, whatever order they were added in: the same views added the
/// other way round make the same drawing and save as the same bytes. A file that lists them in
/// another order (saved before views went by id, or edited by hand) reads by id and saves as those
/// bytes. An edit that leaves them in another order breaks the record's rule.
#[test]
fn views_are_listed_by_id_whatever_order_they_were_added_in() {
    let (d, _, added) = three();
    let mut by_id = added.to_vec();
    by_id.sort_unstable();
    assert_ne!(by_id, added, "the seed's ids aren't in the order the views were added in");
    let listed = |d: &Drawing| d.views.iter().map(|v| v.id).collect::<Vec<_>>();
    let saved: Vec<u64> = file(&d)["views"].as_array().unwrap().iter().map(|v| ids::decode(v["id"].as_str().unwrap()).unwrap()).collect();
    assert_eq!((listed(&d), saved), (by_id.clone(), by_id.clone()));
    let mut reversed = d.clone();
    reversed.views.clear();
    reversed.add_views(d.views.iter().rev().cloned());
    assert_eq!(reversed, d);
    let bytes = d.to_json().unwrap();
    assert_eq!(reversed.to_json().unwrap(), bytes);
    let mut other_order = file(&d);
    other_order["views"].as_array_mut().unwrap().reverse();
    let reopened = Drawing::from_json(&other_order.to_string()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!((listed(&reopened), reopened.to_json().unwrap()), (by_id, bytes));
    let mut unsorted = d.clone();
    unsorted.views.reverse();
    assert_eq!(unsorted.validate().unwrap_err().to_string(), "a drawing lists its views by id");
}

/// What reading refuses, each rule by name: where a view is and what it draws, its parents, its
/// place and margin, its settings, its id, and how many views there are.
#[test]
fn the_views_rules_are_kept_on_reading() {
    let (mut d, mut id_source, [front, right, top]) = three();
    let [front_id, right_id, top_id] = [front, right, top].map(ids::encode);
    let nothing = ids::encode(99);
    let rule = |id: &str, rule: &str| format!("damaged drawing: view {id}: {rule}");
    // The file lists the views by id: front ([0]), top ([1]), then right ([2]).
    // Where it is and what it draws.
    let on = format!("it is on sheet {nothing}, which the drawing doesn't have");
    assert_eq!(refused(&d, |v| v["views"][2]["sheet"] = json!(nothing)), rule(&right_id, &on));
    let draws = format!("it draws source {nothing}, which the drawing doesn't have");
    assert_eq!(refused(&d, |v| v["views"][0]["kind"]["base"]["source"] = json!(nothing)), rule(&front_id, &draws));
    let no_scale = refused(&d, |v| {
        v["views"][0].as_object_mut().unwrap().remove("settings");
    });
    assert_eq!(no_scale, rule(&front_id, "a base view has a scale"));
    let lined_up = refused(&d, |v| v["views"][0]["place"] = json!({"aligned": 10.0}));
    assert_eq!(lined_up, rule(&front_id, "a base view is placed at a point: it has no parent to line up with"));
    let looking = |direction: Value| refused(&d, |v| v["views"][0]["kind"]["base"]["view"] = json!({"direction": direction, "up": [0.0, 0.0, 1.0]}));
    assert_eq!(looking(json!([0.0, 1.0, 0.0])), "read");
    assert_eq!(looking(json!([0.0, 0.0, 0.0])), rule(&front_id, "drawing view: the view direction is zero or not finite"));
    assert_eq!(looking(json!([0.0, 0.0, -1.0])), rule(&front_id, "drawing view: `up` lies along the view direction: give another up"));
    // Its parents: one that isn't a view, itself, and loops of two and three views.
    let parent = |view: usize, parent: &str| {
        let parent = parent.to_string();
        move |v: &mut Value| v["views"][view]["kind"] = json!({"projected": {"parent": parent, "side": "left"}})
    };
    let missing = format!("its parent {nothing} isn't one of the drawing's views");
    assert_eq!(refused(&d, parent(1, &nothing)), rule(&top_id, &missing));
    // Further up the chain, the view whose parent is missing is named. The top view is read
    // before the right view, so its chain finds the right view's missing parent.
    let further = format!("its chain of parents reaches view {right_id}, whose parent {nothing} isn't one of the drawing's views");
    assert_eq!(refused(&d, parent(2, &nothing)), rule(&top_id, &further));
    assert_eq!(refused(&d, parent(1, &front_id)), "read");
    assert_eq!(refused(&d, parent(1, &top_id)), rule(&top_id, "its chain of parents loops"));
    assert_eq!(refused(&d, parent(0, &right_id)), rule(&front_id, "its chain of parents loops"), "two views");
    assert_eq!(refused(&d, parent(0, &top_id)), rule(&front_id, "its chain of parents loops"), "three views");
    // A view lined up with its parent is on its parent's sheet; one placed apart may be on any.
    let other = d.allocate_id(&mut id_source).unwrap();
    let a4 = drawing_sheet::Sheet::standard(PaperSize::A4, Orientation::Landscape);
    d.sheets.push(Arc::new(Sheet {
        id: other,
        name: "Other".into(),
        sheet: a4,
        border: None,
        title_block: None,
        title_block_overrides: std::collections::BTreeMap::new(),
        dpi: 300.0,
        documents: None,
        excluded: Excluded::default(),
    }));
    let other_id = ids::encode(other);
    assert_eq!(
        refused(&d, |v| v["views"][2]["sheet"] = json!(other_id)),
        rule(&right_id, "a view lined up with its parent is on its parent's sheet")
    );
    assert_eq!(refused(&d, |v| v["views"][1]["sheet"] = json!(other_id)), "read");
    // Its place and its reference margin.
    let off_paper = "its place is at most 5000 mm from the sheet's origin";
    assert_eq!(refused(&d, |v| v["views"][1]["place"] = json!({"at": [5000.0, -5000.0]})), "read");
    assert_eq!(refused(&d, |v| v["views"][1]["place"] = json!({"at": [5000.001, 0.0]})), rule(&top_id, off_paper));
    assert_eq!(refused(&d, |v| v["views"][2]["place"] = json!({"aligned": -5000.001})), rule(&right_id, off_paper));
    for (margin, read) in [(0.0, true), (100_000.0, true), (-0.001, false), (100_000.001, false)] {
        let why = refused(&d, |v| v["views"][1]["reference_margin_mm"] = json!(margin));
        let expected = if read { "read".to_string() } else { rule(&top_id, "its reference margin is 0 to 100000 model millimetres") };
        assert_eq!(why, expected, "{margin}");
    }
    // Its settings and its fields, only those it knows; a resolution is the sheet's, not a view's.
    let unknown: [fn(&mut Value); 5] = [
        |v| v["views"][0]["label"] = json!("A"),
        |v| v["views"][0]["settings"]["dpi"] = json!(300.0),
        |v| v["views"][0]["kind"]["base"]["scale"] = json!("1:1"),
        |v| v["views"][1]["kind"]["projected"]["aligned"] = json!(true),
        |v| v["views"][0]["place"] = json!({"at": [1.0, 2.0], "aligned": 3.0}),
    ];
    for (i, edit) in unknown.into_iter().enumerate() {
        assert!(refused(&d, edit).starts_with("damaged drawing: "), "{i}");
    }
    assert!(refused(&d, |v| v["views"][0]["settings"]["scale"] = json!("1:0")).starts_with("damaged drawing: "));
    // Its id is the drawing's: never another object's.
    let sheet_id = ids::encode(d.sheets[0].id);
    assert_eq!(refused(&d, |v| v["views"][2]["id"] = json!(front_id)), format!("damaged drawing: two objects have the id {front_id}"));
    assert_eq!(refused(&d, |v| v["views"][2]["id"] = json!(sheet_id)), format!("damaged drawing: two objects have the id {sheet_id}"));
    // At most MAX_VIEWS views.
    let many = |n: usize| {
        let mut v = file(&d);
        let one = v["views"][0].clone();
        v["views"] = Value::Array(
            (1..=n as u64)
                .map(|i| {
                    let mut view = one.clone();
                    view["id"] = json!(ids::encode(i));
                    view
                })
                .collect(),
        );
        Drawing::from_json(&v.to_string()).map(|d| d.views.len()).map_err(|e| e.to_string())
    };
    assert_eq!(many(MAX_VIEWS), Ok(MAX_VIEWS));
    assert_eq!(many(MAX_VIEWS + 1), Err(format!("damaged drawing: a drawing has at most {MAX_VIEWS} views")));
}

/// A chain of views is at most MAX_VIEW_DEPTH views, the base view one of them.
#[test]
fn a_chain_of_views_is_at_most_its_depth() {
    let (mut d, mut id_source, front) = front();
    let mut last = front;
    for _ in 1..MAX_VIEW_DEPTH {
        last = add(&mut d, &mut id_source, projected(last, ProjectedSide::Right), ViewPlace::At([10.0, 10.0]), ViewOverrides::default());
    }
    assert_eq!(d.views.len(), MAX_VIEW_DEPTH);
    d.validate().unwrap();
    let deepest = add(&mut d, &mut id_source, projected(last, ProjectedSide::Right), ViewPlace::At([10.0, 10.0]), ViewOverrides::default());
    let why = d.validate().unwrap_err().to_string();
    assert_eq!(why, format!("view {}: a chain of views is at most {MAX_VIEW_DEPTH} views", ids::encode(deepest)));
}

/// A view resolves through its chain of parents: its basis turned down the chain in the drawing's
/// projection, each setting from the nearest view that sets it, then the default, saying who gave
/// it, and its own reference margin.
#[test]
fn a_view_resolves_through_its_chain() {
    let (mut d, _, [front, right, top]) = three();
    let view = view_mut(&mut d, top);
    view.settings.tangent_edges = Some(TangentEdges::Show);
    view.reference_margin_mm = 2.0;
    let resolved = d.resolve_view(top).unwrap();
    assert_eq!(resolved.source, d.sources[0].id);
    let right_basis = projected_basis(&NamedView::Front.basis(), ProjectedSide::Right, Projection::Third).unwrap();
    assert_eq!(resolved.basis, projected_basis(&right_basis, ProjectedSide::Above, Projection::Third).unwrap());
    assert_eq!(resolved.scale, Setting { value: "1:2".parse().unwrap(), set_by: SetBy::Parent(front) });
    assert_eq!(resolved.display_style, Setting { value: VisualStyle::VisibleHidden, set_by: SetBy::Parent(right) });
    assert_eq!(resolved.tangent_edges, Setting { value: TangentEdges::Show, set_by: SetBy::This });
    assert_eq!(resolved.reference_style, Setting { value: VisualStyle::Visible, set_by: SetBy::Default });
    assert_eq!(resolved.reference_hides_model, Setting { value: false, set_by: SetBy::Default });
    assert_eq!(resolved.reference_margin_mm, 2.0);
    // The base view gives its own; its projected views follow the drawing's projection.
    let base = d.resolve_view(front).unwrap();
    assert_eq!((base.basis, base.scale.set_by, base.display_style.set_by), (NamedView::Front.basis(), SetBy::This, SetBy::Default));
    assert_eq!(d.resolve_view(right).unwrap().basis, NamedView::Right.basis());
    d.projection = Projection::First;
    assert_eq!(d.resolve_view(right).unwrap().basis, NamedView::Left.basis());
    // A view's own setting wins over its parent's.
    view_mut(&mut d, top).settings.display_style = Some(VisualStyle::Visible);
    assert_eq!(d.resolve_view(top).unwrap().display_style, Setting { value: VisualStyle::Visible, set_by: SetBy::This });
    // Four quarter turns the same way bring the front view back.
    let (mut d, mut id_source, front) = self::front();
    let mut last = front;
    for _ in 0..4 {
        last = add(&mut d, &mut id_source, projected(last, ProjectedSide::Right), ViewPlace::At([10.0, 10.0]), ViewOverrides::default());
    }
    let turned = d.resolve_view(last).unwrap().basis;
    let close = |a: Vec3, b: Vec3| (a - b).len() < 1e-12;
    assert!(close(turned.x, Vec3::X) && close(turned.y, Vec3::Z), "{turned:?}");
    assert_eq!(d.resolve_view(99).unwrap_err().to_string(), format!("the drawing has no view {}", ids::encode(99)));
}

/// A view placed at a point is there; one lined up with its parent takes its own place along the
/// free axis and its parent's centre across it, down a chain, and follows when the parent moves;
/// one placed apart stays. A base view lined up with nothing has no place, nor its views.
#[test]
fn a_views_centre_is_its_point_or_lined_up_with_its_parent() {
    let (mut d, mut id_source, [front, right, top]) = three();
    let back = add(&mut d, &mut id_source, projected(right, ProjectedSide::Right), ViewPlace::Aligned(450.0), ViewOverrides::default());
    let below = add(&mut d, &mut id_source, projected(right, ProjectedSide::Below), ViewPlace::Aligned(60.0), ViewOverrides::default());
    d.validate().unwrap();
    let centres = |d: &Drawing| [front, right, top, back, below].map(|id| d.view_centre(id).unwrap());
    let at = |x, y| Vec2::new(x, y);
    assert_eq!(centres(&d), [at(150.0, 150.0), at(320.0, 150.0), at(320.0, 250.0), at(450.0, 150.0), at(320.0, 60.0)]);
    view_mut(&mut d, front).place = ViewPlace::At([140.0, 130.0]);
    assert_eq!(centres(&d), [at(140.0, 130.0), at(320.0, 130.0), at(320.0, 250.0), at(450.0, 130.0), at(320.0, 60.0)]);
    view_mut(&mut d, front).place = ViewPlace::Aligned(10.0);
    for id in [front, right, top, back, below] {
        assert_eq!(d.view_centre(id).unwrap_err().to_string(), format!("base view {} isn't placed at a point", ids::encode(front)));
    }
    assert_eq!(d.view_centre(99).unwrap_err().to_string(), format!("the drawing has no view {}", ids::encode(99)));
}
