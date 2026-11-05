use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_doc::MAX_PLACEMENT_DEPTH;
use solidcraft_doc::drawing::DEFAULT_DPI;
use solidcraft_doc::drawing::view::{View, ViewKind, ViewPlace};
use solidcraft_doc::drawing_view::{ViewDirection, ViewOverrides};
use solidcraft_geom::cell::Projection;
use solidcraft_geom::drawing_view::{NamedView, ProjectedSide};
use solidcraft_geom::paper::line::{LineGroup, LineKind};
use solidcraft_geom::paper::{NoPictures, ObjectKind};

use super::*;
use crate::schema::drawing_scene::SheetSceneResult;
use crate::schema::output_schema_for;
use crate::schema::tests::conforms;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap()
}

/// A block 100 x 50 x 20 mm along X, Y and Z from the origin, and a drawing of it on A3 with the
/// ISO border and no title block, so its scene is the border and the views: the front view at 1:2
/// centred at (150, 150), and the view above it, the top view in third angle, lined up with it at
/// y 220. The session, the sheet and the two views' ids.
pub(crate) fn front_and_top() -> (Session, u64, [u64; 2]) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 50, "height": 20, "body_name": "Block"}));
    front_and_top_of(s)
}

/// [`front_and_top`]'s drawing of the design `s`.
fn front_and_top_of(mut s: Session) -> (Session, u64, [u64; 2]) {
    run(&mut s, "drawing.create", json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    Arc::make_mut(&mut d.sheets[0]).set_title_block(None);
    let mut id_source = solidcraft_doc::ids::IdSource::seeded(4890);
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    let mut add = |d: &mut Drawing, kind: ViewKind, place: ViewPlace, settings: ViewOverrides| {
        let id = d.allocate_id(&mut id_source).unwrap();
        d.add_views([Arc::new(View { id, sheet, kind, place, settings, reference_shown_percent: 0.0 })]);
        id
    };
    let half = ViewOverrides { scale: Some("1:2".parse().unwrap()), ..ViewOverrides::default() };
    let front = add(&mut d, ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) }, ViewPlace::At([150.0, 150.0]), half);
    let top = add(&mut d, ViewKind::Projected { parent: front, side: ProjectedSide::Above }, ViewPlace::Aligned(220.0), ViewOverrides::default());
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    (s, sheet, [front, top])
}

/// Component `L{level}`, placed inside `L{level - 1}` (the design itself at level 0).
pub(crate) fn nest(s: &mut Session, level: u64) {
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
pub(crate) fn two_cubes() -> (Session, [u64; 2]) {
    let mut s = Session::default();
    let created = run(&mut s, "component.create", json!({"name": "Part"}));
    let first = created["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube", "operation": "new"}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [30, 0, 0]}))["occurrence"].as_u64().unwrap();
    (s, [first, second])
}

/// Sheet `sheet` of the drawing open, built at once.
fn built(s: &Session, sheet: u64) -> SheetScene {
    let d = s.drawing("test").unwrap().clone();
    build(s, &d, sheet).unwrap()
}

/// The provider's scene of `sheet` once nothing is waiting.
fn settled(s: &mut Session, sheet: u64) -> SheetView {
    extract(s, sheet).unwrap();
    while step(s) {}
    extract(s, sheet).unwrap()
}

/// The scene as its JSON text: what byte for byte compares.
fn bytes(scene: &Scene) -> String {
    serde_json::to_string(scene).unwrap()
}

/// The drawing border's groups of sheet `sheet` of the drawing open, as its sheet layout draws them.
fn border(s: &Session, sheet: u64) -> Vec<Group> {
    let d = s.drawing("test").unwrap();
    let record = d.sheets.iter().find(|x| x.id == sheet).unwrap();
    record.border.as_ref().unwrap().layout(&record.sheet).unwrap().paper(sheet)
}

/// The box round the ends of `group`'s stroke pieces, and how long they are in all.
fn reach(group: &Group) -> (Vec2, Vec2, f64) {
    let (mut lo, mut hi, mut length) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY), 0.0);
    for item in &group.items {
        let Item::Stroke { path, .. } = item else { continue };
        for seg in path {
            for p in [seg.start(), seg.end()] {
                lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
            }
            length += seg.length();
        }
    }
    (lo, hi, length)
}

/// Whether `a` and `b` are the same strokes in the same order, their pieces' ends within `tol`.
fn same_within(a: &Group, b: &Group, tol: f64) -> bool {
    let near = |s: &Seg2, t: &Seg2| s.start().dist(t.start()) <= tol && s.end().dist(t.end()) <= tol;
    a.items.len() == b.items.len()
        && a.items.iter().zip(&b.items).all(|pair| match pair {
            (Item::Stroke { path: p, closed: c, width_mm: w, .. }, Item::Stroke { path: q, closed: d, width_mm: v, .. }) => {
                c == d && w == v && p.len() == q.len() && p.iter().zip(q).all(|(s, t)| near(s, t))
            }
            _ => false,
        })
}

/// The scene is the sheet's three border groups as its layout draws them, byte for byte, then the
/// two views in id order: the front view round its 50 x 10 mm outline centred at (150, 150), 120 mm
/// of line in all, the top view round 50 x 25 mm centred at (150, 220), 150 mm. The command answers
/// the same scene, matching its schema, and the SVG writer takes it with the border's three groups
/// before the views'.
#[test]
fn a_sheet_is_its_border_then_its_views_by_id() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let SheetScene { scene, views, .. } = built(&s, sheet);
    let border = border(&s, sheet);
    assert_eq!(border.len(), 3);
    assert_eq!(serde_json::to_string(&scene.groups[..3]).unwrap(), serde_json::to_string(&border).unwrap());
    let order = if front < top { [front, top] } else { [top, front] };
    let tail: Vec<(u64, ObjectKind)> = scene.groups[3..].iter().map(|g| (g.object, g.kind)).collect();
    assert_eq!(tail, order.map(|id| (id, ObjectKind::View)));
    for (id, lo, hi, length) in [(front, [125.0, 145.0], [175.0, 155.0], 120.0), (top, [125.0, 207.5], [175.0, 232.5], 150.0)] {
        let (min, max, drawn) = reach(scene.groups.iter().find(|g| g.object == id).unwrap());
        assert!(min.dist(Vec2::from(lo)) < 1e-6 && max.dist(Vec2::from(hi)) < 1e-6 && (drawn - length).abs() < 1e-6, "{id}: {min:?} {max:?} {drawn}");
    }
    let statuses: Vec<(u64, Status)> = views.iter().map(|v| (v.view, v.status.clone())).collect();
    assert_eq!(statuses, order.map(|id| (id, Status::Drawn)));

    let reply = run(&mut s, CMD, json!({"sheet": "Sheet 1"}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!(reply["scene"], serde_json::to_value(&scene).unwrap());
    let replied: Vec<(Value, Value)> = reply["views"].as_array().unwrap().iter().map(|v| (v["view"].clone(), v["status"].clone())).collect();
    assert_eq!(replied, order.map(|id| (json!(ids::encode(id)), json!("drawn"))));

    let (svg, _) = solidcraft_io::paper::paper_svg(&scene.prepare().unwrap(), &NoPictures, DEFAULT_DPI).unwrap();
    let svg = String::from_utf8(svg).unwrap();
    let borders: Vec<usize> = svg.match_indices("data-kind=\"drawing_border\"").map(|(at, _)| at).collect();
    let first_view = svg.find("data-kind=\"view\"").unwrap();
    assert!(borders.len() == 3 && borders.iter().all(|at| *at < first_view), "{svg}");
}

/// Each view's group is the snapshot's for the same view, scale and centre: the same strokes in the
/// same order, their ends within rounding. The scene works a view's lines out centred on it and
/// then moves them; the snapshot maps them to the centre in one step.
#[test]
fn each_view_draws_as_its_snapshot_within_rounding() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let scene = built(&s, sheet).scene;
    // The snapshot works on the model, so with no drawing open.
    run(&mut s, "drawing.close", json!({}));
    for (id, named, at) in [(front, "front", [150.0, 150.0]), (top, "top", [150.0, 220.0])] {
        let v = run(&mut s, "drawing.view.snapshot", json!({"view": named, "scale": "1:2", "position_mm": at, "scene": true}));
        let snapped: Scene = serde_json::from_value(v["scene"].clone()).unwrap();
        let ours = scene.groups.iter().find(|g| g.object == id).unwrap();
        assert!(same_within(ours, &snapped.groups[0], 1e-9), "{id}: {ours:?} {:?}", snapped.groups[0]);
    }
}

/// A view of many lines is drawn whole, as its snapshot is: the block drilled with eight holes and
/// seen from above is its outline and the eight rims, some 30 records.
#[test]
fn a_view_of_many_lines_draws_as_its_snapshot() {
    let (mut s, sheet, [_, top]) = front_and_top();
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    for i in 0..8 {
        let base = [12.5 + 25.0 * (i % 4) as f64, 12.5 + 25.0 * (i / 4) as f64, -1.0];
        run(&mut s, "solid.cylinder", json!({"diameter": 8, "height": 22, "base": base, "operation": "cut", "targets": ["Block"]}));
    }
    let v = run(&mut s, "drawing.view.snapshot", json!({"view": "top", "scale": "1:2", "position_mm": [150.0, 220.0], "scene": true}));
    let snapped: Scene = serde_json::from_value(v["scene"].clone()).unwrap();
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let scene = built(&s, sheet);
    let state = scene.views.iter().find(|v| v.view == top).unwrap();
    assert_eq!(state.status, Status::Drawn, "{state:?}");
    let ours = scene.scene.groups.iter().find(|g| g.object == top).unwrap();
    assert!(ours.records() > 20 && same_within(ours, &snapped.groups[0], 1e-9), "{ours:?} {:?}", snapped.groups[0]);
}

/// A sheet's title block is drawn last, over its views, as its layout draws it, the sheet its
/// object, and the command's reply still matches its schema. Taking it away is a new scene from the
/// provider, and projects no view again.
#[test]
fn a_sheets_title_block_is_drawn_after_its_views() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let block = solidcraft_geom::title_block::default_block();
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block": serde_json::to_value(&block).unwrap()}));
    let scene = built(&s, sheet).scene;
    let order = if front < top { [front, top] } else { [top, front] };
    let mut expected: Vec<(u64, ObjectKind)> = border(&s, sheet).iter().map(|g| (g.object, g.kind)).collect();
    expected.extend(order.map(|id| (id, ObjectKind::View)));
    expected.push((sheet, ObjectKind::TitleBlock));
    assert_eq!(scene.groups.iter().map(|g| (g.object, g.kind)).collect::<Vec<_>>(), expected);
    let d = s.drawing("test").unwrap();
    let record = d.sheets.iter().find(|x| x.id == sheet).unwrap();
    let drawn = block.layout(&record.border.as_ref().unwrap().layout(&record.sheet).unwrap()).unwrap().paper(sheet, Projection::Third);
    assert_eq!(serde_json::to_string(scene.groups.last().unwrap()).unwrap(), serde_json::to_string(&drawn).unwrap());
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    let before = settled(&mut s, sheet);
    assert_eq!(bytes(&before.scene), bytes(&scene));
    let projections = s.drawing_scenes.projections;
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block": null}));
    let after = extract(&mut s, sheet).unwrap();
    assert!(after.generation > before.generation && after.scene.groups.iter().all(|g| g.kind != ObjectKind::TitleBlock));
    assert_eq!((after.stale.len(), s.drawing_scenes.projections), (0, projections));
}

/// The title block's symbol follows the drawing's projection: the first scene after a change of
/// projection already has the block drawn in it, while the projected view still waits to be drawn
/// again, and once it is, the scene is the one built at once.
#[test]
fn the_title_blocks_symbol_follows_the_drawings_projection() {
    let (mut s, sheet, _) = front_and_top();
    let block = solidcraft_geom::title_block::default_block();
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block": serde_json::to_value(&block).unwrap()}));
    let block_of = |s: &Session, projection: Projection| {
        let d = s.drawing("test").unwrap();
        let record = d.sheets.iter().find(|x| x.id == sheet).unwrap();
        let border = record.border.as_ref().unwrap().layout(&record.sheet).unwrap();
        serde_json::to_string(&record.title_block.as_ref().unwrap().layout(&border).unwrap().paper(sheet, projection)).unwrap()
    };
    let third = settled(&mut s, sheet);
    assert_eq!(serde_json::to_string(third.scene.groups.last().unwrap()).unwrap(), block_of(&s, Projection::Third));
    run(&mut s, "drawing.set", json!({"projection": "first"}));
    let at_once = extract(&mut s, sheet).unwrap();
    assert!(at_once.generation > third.generation && !at_once.stale.is_empty());
    assert_eq!(serde_json::to_string(at_once.scene.groups.last().unwrap()).unwrap(), block_of(&s, Projection::First));
    let first = settled(&mut s, sheet);
    assert_ne!(block_of(&s, Projection::First), block_of(&s, Projection::Third));
    assert_eq!(bytes(&first.scene), bytes(&built(&s, sheet).scene));
}

/// A sheet with no views is its border alone.
#[test]
fn a_sheet_with_no_views_is_its_border() {
    let (mut s, _, _) = front_and_top();
    let second = run(&mut s, "drawing.sheet.add", json!({}))["sheet"].as_str().and_then(ids::decode).unwrap();
    let SheetScene { scene, views, .. } = built(&s, second);
    assert_eq!((serde_json::to_string(&scene.groups).unwrap(), views.len()), (serde_json::to_string(&border(&s, second)).unwrap(), 0));
}

/// A view in `visible_hidden` is drawn as its snapshot is, its hidden lines dashed under its outline,
/// and a sheet's provider sees the style's change as any other's: the last lines show until the view
/// is drawn again, and it ends at the built scene. The pocket is 40 x 10 mm into the back face, so
/// the front view has four hidden strokes.
#[test]
fn a_view_with_hidden_lines_draws_them_as_its_snapshot_does() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [30, 40, 5], "operation": "cut", "targets": ["Block"]}));
    run(&mut s, "drawing.open", json!({"drawing": drawing.clone()}));
    let outline = settled(&mut s, sheet);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"visual_style": "visible_hidden"}}));
    let waiting = extract(&mut s, sheet).unwrap();
    assert_eq!((waiting.stale.contains(&front), bytes(&waiting.scene)), (true, bytes(&outline.scene)));
    let scene = settled(&mut s, sheet).scene;
    assert_eq!(bytes(&scene), bytes(&built(&s, sheet).scene));
    let ours = scene.groups.iter().find(|g| g.object == front).unwrap().clone();
    let dashed = ours.items.iter().filter(|item| matches!(item, Item::Stroke { pattern: Some(_), .. })).count();
    assert_eq!(dashed, 4, "{ours:?}");
    run(&mut s, "drawing.close", json!({}));
    let v = run(
        &mut s,
        "drawing.view.snapshot",
        json!({"view": "front", "scale": "1:2", "position_mm": [150, 150], "scene": true, "visual_style": "visible_hidden"}),
    );
    let snapped: Scene = serde_json::from_value(v["scene"].clone()).unwrap();
    assert!(same_within(&ours, &snapped.groups[0], 1e-9), "{ours:?} {:?}", snapped.groups[0]);
}

/// A sheet's view draws each part's hidden lines by that part's own style. The reference part sits
/// 10 mm behind the model part and 10 mm to its right, so from the front the model hides part of it:
/// those lines are dashed when the reference style draws hidden lines, whatever the model's style,
/// and not when only the model's does (nothing of the model is hidden).
#[test]
fn a_sheets_view_dashes_each_parts_hidden_lines_by_its_own_style() {
    let mut design = Session::default();
    let mut part = |name: &str, body: &str| {
        let occurrence = run(&mut design, "component.create", json!({"name": name}))["occurrence"].as_u64().unwrap();
        run(&mut design, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": body, "operation": "new"}));
        run(&mut design, "component.activate", json!({"component": 0}));
        occurrence
    };
    part("Front", "BodyFront");
    let behind = part("Behind", "BodyBehind");
    run(&mut design, "occurrence.move", json!({"occurrence": behind, "translate": [10, 10, 0]}));
    run(&mut design, "occurrence.bom_type", json!({"occurrence": behind, "bom_type": "reference"}));
    let (mut s, sheet, [front, _]) = front_and_top_of(design);
    let mut dashed = |display: &str, reference: &str| {
        run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"visual_style": display, "reference_style": reference}}));
        let scene = settled(&mut s, sheet).scene;
        let ours = scene.groups.iter().find(|g| g.object == front).unwrap();
        ours.items.iter().filter(|item| matches!(item, Item::Stroke { pattern: Some(_), .. })).count()
    };
    assert!(dashed("visible", "visible_hidden") > 0);
    assert_eq!(dashed("visible_hidden", "visible"), 0);
    assert_eq!(dashed("visible", "visible"), 0);
}

/// A view in a style the paper scene can't draw yet fails with the snapshot's reason, and a view
/// with nothing to measure is a placeholder; each draws nothing, and the sheet's other views and
/// its border are drawn all the same. The command says why for each.
#[test]
fn a_view_that_cant_be_drawn_leaves_out_only_itself() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let status = |s: &Session, id: u64| built(s, sheet).views.into_iter().find(|v| v.view == id).map(|v| v.status);
    let why = "not supported yet: shaded views with lines need the view's exact lines (W06)";
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded_visible"}}));
    assert_eq!((status(&s, front), status(&s, top)), (Some(Status::Drawn), Some(Status::Failed(why.into()))));
    let objects: Vec<u64> = built(&s, sheet).scene.groups.iter().map(|g| g.object).collect();
    assert_eq!(objects, [sheet, sheet, sheet, front]);
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    let failed = reply["views"].as_array().unwrap().iter().find(|v| v["view"] == ids::encode(top)).unwrap().clone();
    assert_eq!((&failed["status"], failed["reason"].as_str().map(|r| r.contains("exact lines"))), (&json!("failed"), Some(true)), "{reply}");
    // Past the walk limit the front view has nothing to measure.
    for level in 0..=MAX_PLACEMENT_DEPTH as u64 + 1 {
        nest(&mut s, level);
    }
    assert_eq!(status(&s, front), Some(Status::Placeholder(Unmeasured::PastWalkLimit)));
    assert_eq!(built(&s, sheet).scene.groups.len(), 3);
}

/// Neither the command nor the provider evaluates the model.
#[test]
fn drawing_a_sheet_evaluates_no_model() {
    let (mut s, sheet, _) = front_and_top();
    let before = s.evaluations;
    run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    settled(&mut s, sheet);
    assert_eq!(s.evaluations, before);
}

/// An unknown sheet and another drawing's sheet are refused, and so is a sheet with no drawing open.
#[test]
fn unknown_and_other_drawings_sheets_are_refused() {
    let (mut s, sheet, _) = front_and_top();
    let refused = |s: &mut Session, named: &str| s.execute(CMD, &json!({"sheet": named})).unwrap_err().to_string();
    assert!(refused(&mut s, "Sheet 9").contains("no sheet `Sheet 9`"));
    run(&mut s, "drawing.create", json!({"name": "Second"}));
    let first = ids::encode(sheet);
    assert!(refused(&mut s, &first).contains(&format!("no sheet `{first}`")));
    assert!(extract(&mut s, sheet).unwrap_err().to_string().contains("the drawing has no sheet"));
    run(&mut s, "drawing.close", json!({}));
    assert!(refused(&mut s, &first).contains("open a drawing first"));
    assert!(extract(&mut s, sheet).is_err() && !step(&mut s));
}

/// A new cache gives the border with both views out of date and draws nothing; each step draws one
/// view, in id order; once nothing is waiting the scene is the built one, byte for byte. The
/// generation goes up each time the scene changes, and only then.
#[test]
fn the_provider_draws_a_view_a_step_until_it_is_the_built_scene() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let order = if front < top { [front, top] } else { [top, front] };
    let whole = built(&s, sheet).scene;
    let first = extract(&mut s, sheet).unwrap();
    assert_eq!(
        (bytes(&first.scene), first.stale.as_slice(), first.views.len()),
        (bytes(&Scene::new(whole.size_mm, whole.groups[..3].to_vec())), &order[..], 0)
    );
    let again = extract(&mut s, sheet).unwrap();
    assert!(Arc::ptr_eq(&again.scene, &first.scene) && again.generation == first.generation);
    assert_eq!(s.drawing_scenes.projections, 0);

    assert!(step(&mut s));
    let one = extract(&mut s, sheet).unwrap();
    let objects: Vec<u64> = one.scene.groups.iter().map(|g| g.object).collect();
    assert_eq!((objects, one.stale.as_slice()), (vec![sheet, sheet, sheet, order[0]], &order[1..]));
    assert!(one.generation > first.generation);
    assert!(!step(&mut s));
    let both = extract(&mut s, sheet).unwrap();
    assert_eq!((bytes(&both.scene), both.stale.len()), (bytes(&whole), 0));
    assert!(both.generation > one.generation);
    assert!(both.views.iter().all(|v| v.status == Status::Drawn) && both.views.len() == 2);
    assert!(!step(&mut s));
    assert_eq!((s.drawing_scenes.projections, extract(&mut s, sheet).unwrap().generation), (2, both.generation));
}

/// Moving a view moves its lines and projects nothing: the top view is lined up with the front
/// one, so both move 50 mm right, and the scene is the built one. Undoing the move projects nothing
/// either and gives back the scene before it, byte for byte.
#[test]
fn a_move_and_its_undo_project_nothing() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let before = settled(&mut s, sheet);
    let projections = s.drawing_scenes.projections;
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"place": {"at": [200.0, 150.0]}}}));
    let after = extract(&mut s, sheet).unwrap();
    assert_eq!((after.stale.len(), s.drawing_scenes.projections), (0, projections));
    assert!(after.generation > before.generation);
    assert_eq!(bytes(&after.scene), bytes(&built(&s, sheet).scene));
    for (was, now) in before.scene.groups.iter().zip(&after.scene.groups).skip(3) {
        assert!(same_within(now, &moved(was, Vec2::new(50.0, 0.0)), 1e-9), "{now:?}");
    }
    run(&mut s, "edit.undo", json!({}));
    let undone = extract(&mut s, sheet).unwrap();
    assert_eq!((undone.stale.len(), s.drawing_scenes.projections), (0, projections));
    assert_eq!(bytes(&undone.scene), bytes(&before.scene));
}

/// A view's picture moves with its lines: its rectangle by exactly the offset, its pixels and
/// opacity as they were.
#[test]
fn a_moved_group_moves_its_picture() {
    let picture = |min: Vec2, max: Vec2| Item::Picture(Picture { source: 3, rect: Aabb2 { min, max }, opacity: 0.5 });
    let group = Group { object: 7, kind: ObjectKind::View, items: vec![picture(Vec2::new(1.0, 2.0), Vec2::new(11.0, 7.0))] };
    assert_eq!(moved(&group, Vec2::new(50.0, -2.0)).items, [picture(Vec2::new(51.0, 0.0), Vec2::new(61.0, 5.0))]);
}

/// After a model change (going back to a checkpoint taken before a tower was added) both views are
/// out of date and the last scene still shows. Each step draws one view from the model as it is
/// now, and a view listed as up to date always draws as the built scene does.
#[test]
fn after_a_model_change_the_last_scene_shows_until_its_views_are_drawn_again() {
    let (mut s, sheet, _) = front_and_top();
    let drawing = ids::encode(s.open_drawing.unwrap());
    let checkpoint = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 40, "corner": [40, 60, 0], "body_name": "Tower", "operation": "new"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let with_tower = settled(&mut s, sheet);
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint": checkpoint}));
    let projections = s.drawing_scenes.projections;
    let old = extract(&mut s, sheet).unwrap();
    assert_eq!((old.stale.len(), bytes(&old.scene), old.generation), (2, bytes(&with_tower.scene), with_tower.generation));
    let now = built(&s, sheet).scene;
    let group = |scene: &Scene, id: u64| {
        scene.groups.iter().find(|g| g.object == id && g.kind == ObjectKind::View).map(|g| serde_json::to_string(g).unwrap())
    };
    loop {
        let more = step(&mut s);
        let mid = extract(&mut s, sheet).unwrap();
        for v in mid.views.iter().filter(|v| !mid.stale.contains(&v.view)) {
            assert_eq!(group(&mid.scene, v.view), group(&now, v.view), "{}", v.view);
        }
        if !more {
            break;
        }
    }
    let last = extract(&mut s, sheet).unwrap();
    assert_eq!((last.stale.len(), s.drawing_scenes.projections, bytes(&last.scene)), (0, projections + 2, bytes(&now)));
}

/// Closing the drawing drops its scenes and the world states they hold; opened again, every view
/// is out of date.
#[test]
fn closing_the_drawing_drops_its_scenes() {
    let (mut s, sheet, _) = front_and_top();
    let drawing = ids::encode(s.open_drawing.unwrap());
    let before = settled(&mut s, sheet);
    assert_eq!(s.drawing_scenes.views.len(), 2);
    run(&mut s, "drawing.close", json!({}));
    let cache = &s.drawing_scenes;
    assert!(cache.views.is_empty() && cache.last.is_none() && cache.shown.is_none() && cache.drawing.is_none());
    assert!(!step(&mut s));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let reopened = extract(&mut s, sheet).unwrap();
    // A new scene, so a new number: the page can't take it for the one it showed before.
    assert!(reopened.stale.len() == 2 && reopened.generation > before.generation);
}

/// A view whose lines would take the sheet past a paper scene's records fails and is left out;
/// a view after it that fits is still drawn, up to the limit exactly. A title block's records are
/// kept for it, though it is drawn after the views.
#[test]
fn a_view_past_the_sheets_records_leaves_out_only_itself() {
    let (s, sheet, _) = front_and_top();
    let d = s.drawing("test").unwrap().clone();
    // One stroke of `n` pieces: n + 2 records with its group.
    let lines = |n: usize| {
        let path = (0..n).map(|i| Seg2::Line { a: Vec2::new(i as f64 * 1e-3, 0.0), b: Vec2::new((i + 1) as f64 * 1e-3, 0.0) }).collect();
        let items = vec![Item::Stroke { path, closed: false, width_mm: 0.5, pattern: None }];
        Drawn {
            group: Some(Group { object: n as u64, kind: ObjectKind::View, items }),
            status: Status::Drawn,
            stats: None,
            boundary: None,
            frame: None,
        }
    };
    for title_block in [None, Some(solidcraft_geom::title_block::default_block())] {
        let record = Sheet { title_block, ..(**d.sheets.iter().find(|x| x.id == sheet).unwrap()).clone() };
        let (empty, ..) = assemble(&record, Projection::Third, &[], std::iter::empty::<(u64, Vec2, Drawn)>()).unwrap().finish().unwrap();
        assert_eq!(empty.groups.len(), if record.title_block.is_some() { 4 } else { 3 });
        let left = MAX_RECORDS - empty.groups.iter().map(Group::records).sum::<usize>();
        // The first leaves 20 records; the second needs 32; the third takes the last 20.
        let views = [lines(left - 22), lines(30), lines(18)];
        let centre = Vec2::new(100.0, 100.0);
        let views = views.iter().enumerate().map(|(i, v)| (i as u64, centre, v));
        let (scene, states, ..) = assemble(&record, Projection::Third, &[], views).unwrap().finish().unwrap();
        let statuses: Vec<&'static str> = states.iter().map(|v| v.status.name()).collect();
        assert_eq!(statuses, ["drawn", "failed", "drawn"]);
        let records = scene.groups.iter().map(Group::records).sum::<usize>();
        assert_eq!((scene.groups.len(), records), (empty.groups.len() + 2, MAX_RECORDS));
        assert!(matches!(&states[1].status, Status::Failed(why) if why.contains("put it on another sheet")), "{:?}", states[1]);
    }
}

/// A view at 200:1 asks the mesh for chords no finer than its floor, so it is drawn rather than
/// refused for a chord it has no setting for.
#[test]
fn a_view_past_100_to_1_is_drawn_with_a_coarser_chord() {
    let (mut s, sheet, [front, _]) = front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"scale": "200:1"}}));
    let scene = built(&s, sheet);
    let front = scene.views.iter().find(|v| v.view == front).unwrap();
    assert_eq!(front.status, Status::Drawn, "{front:?}");
}

/// Seeded mixes of moves, setting changes, undos, redos, extracts and steps: once nothing is
/// waiting, the provider's scene is always the built one, byte for byte.
#[test]
fn seeded_edits_and_steps_always_end_at_the_built_scene() {
    let pick = |seed: u64, salt: u64, n: u64| ((seed ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) % n;
    for seed in 1..=24u64 {
        let (mut s, sheet, [front, top]) = front_and_top();
        let edit =
            |s: &mut Session, view: u64, set: Value| s.execute("drawing.view.edit", &json!({"view": ids::encode(view), "set": set})).map(|_| ());
        for i in 0..12u64 {
            let along = 100.0 + 10.0 * pick(seed, 100 + i, 10) as f64;
            let scale = ["1:1", "1:2", "1:5"][pick(seed, 200 + i, 3) as usize];
            let tangent_edges = ["hide", "show"][pick(seed, 300 + i, 2) as usize];
            // Refused edits (nothing to undo, say) change nothing, which the end still checks.
            let _ = match pick(seed, i, 9) {
                0 => edit(&mut s, front, json!({"place": {"at": [along, 150.0]}})),
                1 => edit(&mut s, top, json!({"place": {"aligned": along + 100.0}})),
                2 => edit(&mut s, front, json!({"scale": scale})),
                3 => edit(&mut s, top, json!({"tangent_edges": tangent_edges})),
                4 => s.execute("edit.undo", &json!({})).map(|_| ()),
                5 => s.execute("edit.redo", &json!({})).map(|_| ()),
                6 => {
                    let source = [json!({"occurrences": [[]]}), json!(null)][pick(seed, 400 + i, 2) as usize].clone();
                    edit(&mut s, top, json!({"source": source}))
                }
                7 => {
                    step(&mut s);
                    Ok(())
                }
                _ => extract(&mut s, sheet).map(drop),
            };
        }
        let end = settled(&mut s, sheet);
        assert!(end.stale.is_empty(), "seed {seed}");
        let at_once = built(&s, sheet);
        assert_eq!(bytes(&end.scene), bytes(&at_once.scene), "seed {seed}");
        assert_eq!(&*end.objects, at_once.objects.as_slice(), "seed {seed}");
        assert_eq!(run(&mut s, "drawing.status", json!({}))["waiting"], json!(0), "seed {seed}");
    }
}

/// Setting a part as reference evaluates nothing, yet the views of it are out of date: its lines
/// join the reference parts', so no longer hide what is under them or stop at its own extents.
#[test]
fn a_change_of_reference_parts_redraws_the_views() {
    let mut design = Session::default();
    let mut part = |name: &str, body: &str| {
        let occurrence = run(&mut design, "component.create", json!({"name": name}))["occurrence"].as_u64().unwrap();
        run(&mut design, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": body, "operation": "new"}));
        run(&mut design, "component.activate", json!({"component": 0}));
        occurrence
    };
    part("Under", "BodyUnder");
    let over = part("Over", "BodyOver");
    run(&mut design, "occurrence.move", json!({"occurrence": over, "translate": [10, 0, 10]}));
    let (mut s, sheet, [front, top]) = front_and_top_of(design);
    let before = settled(&mut s, sheet);
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    let in_the_model = |s: &mut Session, id: &str, p: Value| {
        run(s, "drawing.close", json!({}));
        run(s, id, p);
        run(s, "drawing.open", drawing.clone());
    };
    in_the_model(&mut s, "occurrence.bom_type", json!({"occurrence": over, "bom_type": "reference"}));
    let stale = extract(&mut s, sheet).unwrap().stale;
    assert!(stale.len() == 2 && stale.contains(&front) && stale.contains(&top), "{stale:?}");
    let after = settled(&mut s, sheet);
    assert_ne!(bytes(&before.scene), bytes(&after.scene));
    let thin = |scene: &Scene| {
        let items = scene.groups.iter().filter(|g| g.kind == ObjectKind::View).flat_map(|g| &g.items);
        items.filter(|i| matches!(i, Item::Stroke { width_mm, .. } if *width_mm == LineKind::Thin.width_mm(LineGroup::DEFAULT))).count()
    };
    assert_eq!(thin(&before.scene), 0);
    assert!(thin(&after.scene) > 0);
    let scene = built(&s, sheet);
    assert_eq!(bytes(&scene.scene), bytes(&after.scene), "the provider and a build draw one thing");
    // Undone, the views draw again, as they were; redone, as they were then.
    in_the_model(&mut s, "edit.undo", json!({}));
    assert_eq!(extract(&mut s, sheet).unwrap().stale.len(), 2);
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&before.scene));
    in_the_model(&mut s, "edit.redo", json!({}));
    assert_eq!(extract(&mut s, sheet).unwrap().stale.len(), 2);
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&after.scene));
    // Set on its component, it draws the same.
    in_the_model(&mut s, "occurrence.bom_type", json!({"occurrence": over, "bom_type": null}));
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&before.scene));
    in_the_model(&mut s, "component.metadata.set", json!({"component": "Over", "bom_type": "reference"}));
    assert_eq!(extract(&mut s, sheet).unwrap().stale.len(), 2);
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&after.scene));
    // A saved view takes the setting: the reference part now covers what is under it.
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"reference_hides_model": true}}));
    assert_ne!(bytes(&settled(&mut s, sheet).scene), bytes(&after.scene));
}

/// A run of text moves with its group: its start moves, and nothing else changes.
#[test]
fn text_moves_with_its_group() {
    let run = TextRun {
        font: [0; 32],
        scale: 0.005,
        origin: Vec2::new(1.0, 2.0),
        angle: 0.5,
        slant: 0.2,
        glyphs: vec![],
        text: String::new(),
        clusters: vec![],
        rtl: false,
    };
    let group = Group { object: 1, kind: ObjectKind::View, items: vec![Item::Text(run.clone())] };
    let Item::Text(placed) = &moved(&group, Vec2::new(10.0, 20.0)).items[0] else { panic!("a run") };
    assert_eq!(placed, &TextRun { origin: Vec2::new(11.0, 22.0), ..run });
}

/// A saved view's reference_shown_percent redraws that view alone, grows its boundary on the reference
/// part's side, leaves the model's strokes where they were, and goes back with an undo.
#[test]
fn a_saved_views_reference_shown_percent_grows_only_what_it_should() {
    let mut design = Session::default();
    let mut part = |name: &str, body: &str| {
        let occurrence = run(&mut design, "component.create", json!({"name": name}))["occurrence"].as_u64().unwrap();
        run(&mut design, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": body, "operation": "new"}));
        run(&mut design, "component.activate", json!({"component": 0}));
        occurrence
    };
    part("Under", "BodyUnder");
    let over = part("Over", "BodyOver");
    run(&mut design, "occurrence.move", json!({"occurrence": over, "translate": [10, 0, 10]}));
    run(&mut design, "occurrence.bom_type", json!({"occurrence": over, "bom_type": "reference"}));
    let (mut s, sheet, [_, top]) = front_and_top_of(design);
    let strokes = |scene: &Scene, reference: bool| {
        let items = scene.groups.iter().filter(|g| g.kind == ObjectKind::View).flat_map(|g| &g.items);
        let thin = |i: &&Item| matches!(i, Item::Stroke { width_mm, .. } if *width_mm == LineKind::Thin.width_mm(LineGroup::DEFAULT));
        let mut drawn: Vec<String> = items.filter(|i| matches!(i, Item::Stroke { .. }) && thin(i) == reference).map(|i| format!("{i:?}")).collect();
        drawn.sort();
        drawn
    };
    let before = settled(&mut s, sheet);
    let boundary = |s: &mut Session| run(s, "drawing.view.inspect", json!({"view": ids::encode(top)}))["boundary_mm"].clone();
    let (min, max) = {
        let b = boundary(&mut s);
        (b["min"].clone(), b["max"].clone())
    };
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"reference_shown_percent": 100}}));
    assert_eq!(extract(&mut s, sheet).unwrap().stale, [top]);
    let after = settled(&mut s, sheet);
    assert_eq!(strokes(&before.scene, false), strokes(&after.scene, false), "the model stays where it is");
    assert_ne!(strokes(&before.scene, true), strokes(&after.scene, true));
    let grown = boundary(&mut s);
    assert_eq!(grown["min"], min, "the side it doesn't pass stays");
    assert_ne!(grown["max"], max);
    assert_eq!(bytes(&built(&s, sheet).scene), bytes(&after.scene), "the provider and a build draw one thing");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&before.scene));
}

fn near(b: &Aabb2, min: [f64; 2], max: [f64; 2]) -> bool {
    (b.min - Vec2::from(min)).len() < 1e-9 && (b.max - Vec2::from(max)).len() < 1e-9
}

/// A sheet's objects are its border, once, then each view by id. At 2:1 the block's front view is
/// 200 x 40 mm round (150, 150) and its top view 200 x 100 mm round (150, 220); each draws up to its
/// boundary, and inspect says the same boundary. The command answers them beside the scene, not in
/// it, as its schema says.
#[test]
fn a_sheets_objects_say_where_each_one_is() {
    let (mut s, sheet, [front, top]) = front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"scale": "2:1"}}));
    let objects = built(&s, sheet).objects;
    let named: Vec<(u64, ObjectKind)> = objects.iter().map(|o| (o.object, o.kind)).collect();
    let by_id = [front.min(top), front.max(top)];
    assert_eq!(named, [(sheet, ObjectKind::DrawingBorder), (by_id[0], ObjectKind::View), (by_id[1], ObjectKind::View)]);
    let border = objects[0];
    assert_eq!(border.boundary_mm, None);
    assert!(
        border.bounds_mm.min.x >= 0.0 && border.bounds_mm.min.y >= 0.0 && border.bounds_mm.max.x <= 420.0 && border.bounds_mm.max.y <= 297.0,
        "{border:?}"
    );
    for (id, min, max) in [(front, [50.0, 130.0], [250.0, 170.0]), (top, [50.0, 170.0], [250.0, 270.0])] {
        let object = objects[if id == by_id[0] { 1 } else { 2 }];
        assert!(near(&object.bounds_mm, min, max), "{object:?}");
        assert!(near(&object.boundary_mm.unwrap(), min, max), "{object:?}");
        let inspected = run(&mut s, "drawing.view.inspect", json!({"view": ids::encode(id)}));
        let corner = |at: &str| Vec2::new(inspected["boundary_mm"][at][0].as_f64().unwrap(), inspected["boundary_mm"][at][1].as_f64().unwrap());
        assert!(
            (corner("min") - object.boundary_mm.unwrap().min).len() < 1e-9 && (corner("max") - object.boundary_mm.unwrap().max).len() < 1e-9,
            "{inspected}"
        );
    }
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!((&reply["objects"][1]["object"], &reply["objects"][1]["kind"]), (&json!(ids::encode(by_id[0])), &json!("view")), "{reply}");
    assert_eq!(reply["objects"][0].get("boundary_mm"), None, "{reply}");
    assert_eq!(reply["scene"].get("objects"), None, "{reply}");
}

/// A view that can't be drawn can still be picked: a failed one by its boundary, and a placeholder by
/// the 20 x 20 mm round its centre.
#[test]
fn a_view_with_no_lines_is_an_object_at_its_boundary() {
    let (mut s, sheet, [front, top]) = front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded"}}));
    let object = |s: &Session, id: u64| built(s, sheet).objects.into_iter().find(|o| o.object == id).unwrap();
    let failed = object(&s, top);
    assert!(near(&failed.bounds_mm, [125.0, 207.5], [175.0, 232.5]) && failed.boundary_mm == Some(failed.bounds_mm), "{failed:?}");
    for level in 0..=MAX_PLACEMENT_DEPTH as u64 + 1 {
        nest(&mut s, level);
    }
    let held = object(&s, front);
    assert!(near(&held.bounds_mm, [140.0, 140.0], [160.0, 160.0]) && held.boundary_mm == Some(held.bounds_mm), "{held:?}");
}

/// A view's box holds what it draws, whatever that is, and one that drew nothing stands at its
/// boundary rather than at an empty box.
#[test]
fn a_views_box_is_what_it_draws() {
    let (s, ..) = front_and_top();
    let record = s.drawing("test").unwrap().sheets[0].clone();
    let (centre, boundary) = (Vec2::new(100.0, 100.0), Aabb2 { min: Vec2::new(-30.0, -20.0), max: Vec2::new(30.0, 20.0) });
    let rect = Aabb2 { min: Vec2::new(1.0, 2.0), max: Vec2::new(5.0, 7.0) };
    let corners = [rect.min, Vec2::new(rect.max.x, rect.min.y), rect.max, Vec2::new(rect.min.x, rect.max.y)];
    let segs = (0..4).map(|i| Seg2::Line { a: corners[i], b: corners[(i + 1) % 4] }).collect();
    let fill = Item::Fill { region: Region2 { outer: Loop2 { segs }, holes: Vec::new() } };
    let picture = Item::Picture(Picture { source: 1, rect, opacity: 1.0 });
    let moved = [101.0, 102.0];
    let moved_to = [105.0, 107.0];
    for (items, min, max) in [(vec![], [70.0, 80.0], [130.0, 120.0]), (vec![fill], moved, moved_to), (vec![picture], moved, moved_to)] {
        let drawn = Drawn {
            group: Some(Group { object: 7, kind: ObjectKind::View, items }),
            status: Status::Drawn,
            stats: None,
            boundary: Some(boundary),
            frame: None,
        };
        let (_, _, objects, _) = assemble(&record, Projection::Third, &[], [(7, centre, &drawn)].into_iter()).unwrap().finish().unwrap();
        let object = objects.iter().find(|o| o.object == 7).unwrap();
        assert!(near(&object.bounds_mm, min, max), "{object:?}");
    }
}

/// The provider lists a view once it is drawn, moves its bounds with it and projects nothing for
/// that, and a view gone from the drawing is gone from the list.
#[test]
fn the_providers_objects_follow_its_views() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let first = extract(&mut s, sheet).unwrap();
    assert_eq!(first.objects.iter().map(|o| o.object).collect::<Vec<_>>(), [sheet], "only what is drawn so far");
    let drawn = settled(&mut s, sheet);
    let at = |view: &SheetView, id: u64| view.objects.iter().find(|o| o.object == id).copied().unwrap();
    let before = at(&drawn, front);
    let projections = s.drawing_scenes.projections;
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"place": {"at": [160.0, 150.0]}}}));
    let moved = settled(&mut s, sheet);
    assert_eq!(s.drawing_scenes.projections, projections, "a move draws nothing");
    let by = Vec2::new(10.0, 0.0);
    assert!(near(&at(&moved, front).bounds_mm, (before.bounds_mm.min + by).into(), (before.bounds_mm.max + by).into()), "{moved:?}");
    assert!(
        near(&at(&moved, top).bounds_mm, (at(&drawn, top).bounds_mm.min + by).into(), (at(&drawn, top).bounds_mm.max + by).into()),
        "the view lined up with it goes along"
    );
    assert_eq!(&*moved.objects, built(&s, sheet).objects.as_slice());
}

/// `drawing.status`, matching its schema, and each view's status word by id.
fn status_of(s: &mut Session) -> (Value, BTreeMap<u64, String>) {
    let reply = run(s, "drawing.status", json!({}));
    let schema = output_schema_for::<crate::schema::drawing_scene::DrawingStatusResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    let words = reply["views"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (ids::decode(v["view"].as_str().unwrap()).unwrap(), v["status"].as_str().unwrap().to_string()));
    let words = words.collect();
    (reply, words)
}

/// Every word of `words` is `word`.
fn all_are(words: &BTreeMap<u64, String>, word: &str) -> bool {
    !words.is_empty() && words.values().all(|w| w == word)
}

/// A view never drawn is out of date; `step` draws it and it is current. A model edit marks every
/// view; a move marks none, and neither does its undo; a change of a setting marks the view and
/// those following it. Reading draws nothing and changes nothing.
#[test]
fn status_says_which_views_are_out_of_date() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "out_of_date"), "{reply}");
    assert_eq!(reply["waiting"], json!(2), "{reply}");
    extract(&mut s, sheet).unwrap();
    step(&mut s);
    // A step draws the lowest id first, and the seeded ids needn't put the front view there.
    let (first, second) = if front < top { (front, top) } else { (top, front) };
    let (reply, words) = status_of(&mut s);
    assert_eq!((words[&first].as_str(), words[&second].as_str()), ("current", "out_of_date"), "{reply}");
    assert_eq!(reply["waiting"], json!(1), "{reply}");
    settled(&mut s, sheet);
    let (doc, projections) = (s.doc.clone(), s.drawing_scenes.projections);
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "current"), "{reply}");
    assert_eq!(reply["waiting"], json!(0), "{reply}");
    assert!(Arc::ptr_eq(&doc, &s.doc) && s.drawing_scenes.projections == projections, "reading draws nothing");

    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"place": {"at": [160.0, 150.0]}}}));
    assert!(all_are(&status_of(&mut s).1, "current"), "a move marks none");
    run(&mut s, "edit.undo", json!({}));
    assert!(all_are(&status_of(&mut s).1, "current"), "nor its undo");

    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"tangent_edges": "show"}}));
    let words = status_of(&mut s).1;
    assert_eq!((words[&front].as_str(), words[&top].as_str()), ("current", "out_of_date"));
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"scale": "1:5"}}));
    assert!(all_are(&status_of(&mut s).1, "out_of_date"), "the view following it too");
    settled(&mut s, sheet);

    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 40, "corner": [40, 60, 0], "body_name": "Tower", "operation": "new"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert!(all_are(&status_of(&mut s).1, "out_of_date"), "a model edit marks every view");
}

/// A failed or placeholder view always says why, and a view on another sheet is out of date until
/// its own sheet is drawn, with that sheet's id.
#[test]
fn status_says_why_a_view_has_no_lines() {
    let (mut s, sheet, [front, top]) = front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded_visible"}}));
    settled(&mut s, sheet);
    let (reply, words) = status_of(&mut s);
    assert_eq!((words[&front].as_str(), words[&top].as_str()), ("current", "failed"), "{reply}");
    let failed = reply["views"].as_array().unwrap().iter().find(|v| v["status"] == "failed").unwrap();
    assert!(failed["reason"].as_str().unwrap().contains("exact lines"), "{reply}");
    assert_eq!(failed["sheet"], json!(ids::encode(sheet)), "{reply}");

    run(&mut s, "drawing.sheet.add", json!({"name": "Other"}));
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": null}}));
    run(&mut s, "drawing.view.move_to_sheet", json!({"view": ids::encode(top), "sheet": "Other", "break_alignment": true}));
    settled(&mut s, sheet);
    let (reply, words) = status_of(&mut s);
    assert_eq!((words[&front].as_str(), words[&top].as_str()), ("current", "out_of_date"), "{reply}");
    let other = s.drawing("test").unwrap().sheets.iter().find(|x| x.id != sheet).unwrap().id;
    let moved = reply["views"].as_array().unwrap().iter().find(|v| v["view"] == json!(ids::encode(top))).unwrap();
    assert_eq!(moved["sheet"], json!(ids::encode(other)), "{reply}");

    for level in 0..=MAX_PLACEMENT_DEPTH as u64 + 1 {
        nest(&mut s, level);
    }
    settled(&mut s, sheet);
    let reply = status_of(&mut s).0;
    let held = reply["views"].as_array().unwrap().iter().find(|v| v["view"] == json!(ids::encode(front))).unwrap();
    assert_eq!(held["status"], json!("placeholder"), "{reply}");
    assert!(!held["reason"].as_str().unwrap().is_empty(), "{reply}");
}

/// A view that drew but didn't fit its sheet's scene is failed, as that scene left it, until it is
/// drawn again: a scene older than the last drawing is not trusted.
#[test]
fn status_takes_what_the_last_scene_left_out_until_a_view_draws_again() {
    let (mut s, sheet, [front, top]) = front_and_top();
    settled(&mut s, sheet);
    let fitted = overfill(&mut s, top);
    extract(&mut s, sheet).unwrap();
    let (reply, words) = status_of(&mut s);
    assert_eq!((words[&front].as_str(), words[&top].as_str()), ("current", "failed"), "{reply}");
    let failed = reply["views"].as_array().unwrap().iter().find(|v| v["status"] == "failed").unwrap();
    assert!(failed["reason"].as_str().unwrap().contains("put it on another sheet"), "{reply}");

    // Drawn again since that scene, and it fits: its own drawing says where it stands.
    let cache = &mut s.drawing_scenes;
    cache.serial += 1;
    let entry = cache.views.get_mut(&top).unwrap();
    entry.drawn = fitted;
    entry.serial = cache.serial;
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "current"), "{reply}");
}

/// A scene counts only for its own sheet as that sheet is now: a view moved off the sheet whose
/// scene left it out, and a view left there when another goes, each read their own drawing again.
#[test]
fn status_takes_a_scene_only_for_its_sheet_as_it_is() {
    let failed_on_its_sheet = |s: &mut Session, top: u64| {
        let (reply, words) = status_of(s);
        assert_eq!(words[&top], "failed", "{reply}");
    };
    let (mut s, sheet, [_, top]) = front_and_top();
    settled(&mut s, sheet);
    overfill(&mut s, top);
    extract(&mut s, sheet).unwrap();
    failed_on_its_sheet(&mut s, top);
    run(&mut s, "drawing.sheet.add", json!({"name": "Other"}));
    run(&mut s, "drawing.view.move_to_sheet", json!({"view": ids::encode(top), "sheet": "Other", "break_alignment": true}));
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "current"), "moved, it no longer has the old sheet's advice: {reply}");

    let (mut s, sheet, [front, top]) = front_and_top();
    settled(&mut s, sheet);
    overfill(&mut s, top);
    extract(&mut s, sheet).unwrap();
    failed_on_its_sheet(&mut s, top);
    run(&mut s, "drawing.sheet.add", json!({"name": "Other"}));
    run(&mut s, "drawing.view.move_to_sheet", json!({"view": ids::encode(front), "sheet": "Other", "break_alignment": true}));
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "current"), "its sheet changed since that scene: {reply}");
}

/// Gives `view` a drawing of lines past a paper scene's records on their own (drawn, but no sheet
/// holds them), as if just drawn; returns the drawing it had.
fn overfill(s: &mut Session, view: u64) -> Drawn {
    let cache = &mut s.drawing_scenes;
    cache.serial += 1;
    let serial = cache.serial;
    let entry = cache.views.get_mut(&view).unwrap();
    let path = (0..MAX_RECORDS).map(|i| Seg2::Line { a: Vec2::new(i as f64 * 1e-3, 0.0), b: Vec2::new((i + 1) as f64 * 1e-3, 0.0) }).collect();
    let items = vec![Item::Stroke { path, closed: false, width_mm: 0.5, pattern: None }];
    let group = Group { object: view, kind: ObjectKind::View, items };
    entry.serial = serial;
    std::mem::replace(&mut entry.drawn, Drawn { group: Some(group), status: Status::Drawn, stats: None, boundary: None, frame: None })
}

/// Each dimension's word is the one inspect uses: read, then lost when its body goes, with why. A
/// parameter status takes nothing, and needs a drawing open.
#[test]
fn status_says_where_each_dimension_stands() {
    let (mut s, _, [front, _]) = front_and_top();
    let added = run(&mut s, "drawing.dimension.add", json!({"view": ids::encode(front), "kind": {"length": {"edge": [50, 0, 20]}}}));
    let reply = status_of(&mut s).0;
    assert_eq!(reply["dimensions"], json!([{"dimension": added["record"]["id"], "status": "ok"}]), "{reply}");
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    assert!(s.execute("drawing.status", &json!({})).is_err(), "no drawing open");
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.delete", json!({"feature": block.to_string()}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let reply = status_of(&mut s).0;
    let lost = &reply["dimensions"][0];
    assert_eq!(lost["status"], json!("lost"), "{reply}");
    assert!(!lost["why"].as_str().unwrap().is_empty(), "{reply}");
    assert!(s.execute("drawing.status", &json!({"sheet": "x"})).is_err());
}

/// A dimension of the block's top front edge on view `view`, 10 from it; its id.
fn top_edge(s: &mut Session, view: u64) -> u64 {
    let added = run(s, "drawing.dimension.add", json!({"view": ids::encode(view), "kind": {"linear": {"a": [0, 0, 20], "b": [100, 0, 20]}}}));
    ids::decode(added["dimension"].as_str().unwrap()).unwrap()
}

/// `id`'s group in `scene`.
fn group_of(scene: &Scene, id: u64) -> &Group {
    scene.groups.iter().find(|g| g.object == id).unwrap()
}

/// A dimension draws after the views, at paper size and on its view: the top edge of the block at
/// 1:2, centred at (150, 150), has its tips 50 apart, 10 above the edge. `build` and the provider
/// give the same bytes, and its place is listed with the scene's other objects.
#[test]
fn a_dimension_is_drawn_after_the_views_on_the_sheet() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let id = top_edge(&mut s, front);
    let built = built(&s, sheet);
    let group = group_of(&built.scene, id);
    assert_eq!(group.kind, ObjectKind::Dimension);
    assert_eq!(built.scene.groups.last().map(|g| g.object), Some(id));
    let tips: Vec<Vec2> = group
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fill { region } => Some(region.outer.segs[0].start()),
            _ => None,
        })
        .collect();
    assert_eq!(tips.len(), 2);
    assert!(((tips[0].x - tips[1].x).abs() - 50.0).abs() < 1e-9, "{tips:?}");
    assert!(tips.iter().all(|t| (t.y - 165.0).abs() < 1e-9), "{tips:?}");
    let object = built.objects.iter().find(|o| o.object == id).unwrap();
    assert_eq!(object.kind, ObjectKind::Dimension);
    assert_eq!(
        built.dimensions,
        vec![DimensionState { dimension: id, view: front, status: solidcraft_doc::drawing::dimension::Status::Ok, drawn: true, why: None }]
    );
    let provided = settled(&mut s, sheet);
    assert_eq!(bytes(&provided.scene), bytes(&built.scene));
    assert_eq!(&*provided.objects, built.objects.as_slice());
    assert_eq!(provided.dimensions, built.dimensions);
    assert!(provided.stale_dimensions.is_empty());
}

/// Moving a view moves its dimensions, the same bytes moved, with no projection and no new read. An
/// edit of one dimension reads that one alone, and projects nothing.
#[test]
fn a_moved_view_moves_its_dimensions_and_an_edit_reads_one() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let id = top_edge(&mut s, front);
    let other = run(&mut s, "drawing.dimension.add", json!({"view": ids::encode(front), "kind": {"linear": {"a": [0, 0, 0], "b": [0, 0, 20]}}}));
    let other = ids::decode(other["dimension"].as_str().unwrap()).unwrap();
    let before = settled(&mut s, sheet);
    let (projections, serial) = (s.drawing_scenes.projections, s.drawing_scenes.dimensions[&other].serial);

    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"place": {"at": [170.0, 130.0]}}}));
    let after = settled(&mut s, sheet);
    assert_eq!(s.drawing_scenes.projections, projections, "a move projects nothing");
    assert_eq!(s.drawing_scenes.dimensions[&other].serial, serial, "and reads nothing");
    assert_eq!(bytes_of(group_of(&after.scene, id)), bytes_of(&moved(group_of(&before.scene, id), Vec2::new(20.0, -20.0))));

    run(&mut s, "drawing.dimension.edit", json!({"dimension": ids::encode(id), "set": {"offset_mm": 20.0}}));
    let edited = settled(&mut s, sheet);
    assert_eq!(s.drawing_scenes.projections, projections, "an edit projects nothing");
    assert_eq!(s.drawing_scenes.dimensions[&other].serial, serial, "the other dimension isn't read");
    assert_ne!(bytes_of(group_of(&edited.scene, id)), bytes_of(group_of(&after.scene, id)));
    assert_eq!(bytes(&edited.scene), bytes(&built(&s, sheet).scene));
}

fn bytes_of(group: &Group) -> String {
    serde_json::to_string(group).unwrap()
}

/// The block with a hole 12 across through its front, and a dimension of the hole on the front view:
/// the session, the sheet, the view, the dimension and the drawing's id.
fn holed() -> (Session, u64, u64, u64, String) {
    let (mut s, sheet, [front, _]) = front_and_top();
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    let cut = json!({"radius": 6, "height": 52, "base": [50, -1, 10], "axis": "Y", "operation": "cut", "targets": ["Block"], "name": "Hole"});
    run(&mut s, "solid.cylinder", cut);
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let added = run(&mut s, "drawing.dimension.add", json!({"view": ids::encode(front), "kind": {"radial": {"edge": [56, 0, 10]}}}));
    (s, sheet, front, ids::decode(added["dimension"].as_str().unwrap()).unwrap(), drawing)
}

/// The hole deleted, with the drawing closed as a model edit has it.
fn delete_hole(s: &mut Session, drawing: &str) {
    run(s, "drawing.close", json!({}));
    run(s, "timeline.delete", json!({"feature": "Hole"}));
    run(s, "drawing.open", json!({"drawing": drawing}));
}

/// A dimension whose view isn't drawn isn't drawn and says so. A lost one has no lines, is listed,
/// and leaves the sheet drawn; undo finds it again, the same bytes. A model edit that moves what it
/// measures moves its lines.
#[test]
fn a_lost_dimension_leaves_the_sheet_drawn_and_is_listed() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let on_top = top_edge(&mut s, top);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded_visible"}}));
    let view_failed = settled(&mut s, sheet);
    let held = view_failed.dimensions.iter().find(|d| d.dimension == on_top).unwrap();
    assert!(!held.drawn && held.why.as_deref() == Some("its view isn't drawn"), "{held:?}");
    assert!(view_failed.scene.groups.iter().all(|g| g.object != on_top));
    assert_eq!(bytes(&view_failed.scene), bytes(&built(&s, sheet).scene));

    // Taller: the view is drawn again first, then its dimension, at the new place.
    let id = top_edge(&mut s, front);
    let before = settled(&mut s, sheet);
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"height": 40}}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let view = extract(&mut s, sheet).unwrap();
    assert!(view.stale.contains(&front) && view.stale_dimensions.is_empty(), "the views first");
    assert_ne!(bytes_of(group_of(&settled(&mut s, sheet).scene, id)), bytes_of(group_of(&before.scene, id)));

    let (mut s, sheet, front, id, drawing) = holed();
    let before = settled(&mut s, sheet);
    delete_hole(&mut s, &drawing);
    let lost = settled(&mut s, sheet);
    let state = lost.dimensions.iter().find(|d| d.dimension == id).unwrap();
    assert!(!state.drawn && matches!(state.status, solidcraft_doc::drawing::dimension::Status::Lost(..)), "{state:?}");
    assert!(lost.scene.groups.iter().all(|g| g.object != id));
    assert!(lost.scene.groups.iter().any(|g| g.object == front), "the sheet is still drawn");
    assert_eq!(bytes(&lost.scene), bytes(&built(&s, sheet).scene));

    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let again = settled(&mut s, sheet);
    assert_eq!(bytes_of(group_of(&again.scene, id)), bytes_of(group_of(&before.scene, id)), "found again, drawn the same");
}

/// A dimension past a paper scene's records is left out and says so, and the rest of the sheet is
/// still drawn. So is one whose lines no paper scene holds.
#[test]
fn a_dimension_past_the_scenes_limits_is_left_out_and_says_why() {
    let (s, sheet, [front, _]) = front_and_top();
    let d = s.drawing("test").unwrap().clone();
    let record = sheet_of(&d, sheet).unwrap();
    let group = |items| Group { object: front, kind: ObjectKind::View, items };
    let view = Drawn {
        group: Some(group(Vec::new())),
        status: Status::Drawn,
        stats: None,
        boundary: None,
        frame: Some(ViewFrame::new(solidcore_record::Plane::XY, "1:1".parse().unwrap(), Vec2::ZERO, Vec2::ZERO).unwrap()),
    };
    let mut assembly = assemble(record, Projection::Third, &[], std::iter::once((front, Vec2::new(100.0, 100.0), view))).unwrap();
    let stroke = |to: Vec2| Item::Stroke { path: vec![Seg2::Line { a: Vec2::ZERO, b: to }], closed: false, width_mm: 0.25, pattern: None };
    let dimension = |id, items| DrawnDimension {
        dimension: id,
        view: front,
        status: solidcraft_doc::drawing::dimension::Status::Ok,
        group: Some(Group { object: id, kind: ObjectKind::Dimension, items }),
        why: None,
    };
    let too_many = dimension(1, vec![stroke(Vec2::X); MAX_RECORDS + 1]);
    let off_paper = dimension(2, vec![stroke(Vec2::new(1e9, 0.0))]);
    let fits = dimension(3, vec![stroke(Vec2::X)]);
    assembly.add_dimensions([&too_many, &off_paper, &fits].into_iter());
    let (scene, _, objects, states) = assembly.finish().unwrap();
    assert!(!states[0].drawn && states[0].why.as_deref().is_some_and(|w| w.contains("records")), "{:?}", states[0]);
    assert!(!states[1].drawn && states[1].why.is_some(), "{:?}", states[1]);
    assert!(states[2].drawn && states[2].why.is_none(), "{:?}", states[2]);
    assert!(scene.groups.iter().any(|g| g.object == 3) && !scene.groups.iter().any(|g| g.object == 1 || g.object == 2));
    assert!(objects.iter().any(|o| o.object == 3 && o.kind == ObjectKind::Dimension));
}

/// The scene's reply lists its dimensions in its schema, with a dimension's group in the scene, and
/// an export writes the sheet with a lost dimension and reports it.
#[test]
fn replies_list_the_dimensions_and_an_export_reports_a_lost_one() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let id = top_edge(&mut s, front);
    let reply = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!(reply["dimensions"], json!([{"dimension": ids::encode(id), "view": ids::encode(front), "status": "ok", "drawn": true}]));
    let groups = reply["scene"]["groups"].as_array().unwrap();
    assert_eq!(groups.last().unwrap()["kind"], json!("dimension"), "{reply}");
    let exported = run(&mut s, "drawing.export", json!({"sheet": ids::encode(sheet)}));
    assert_eq!(exported["dimensions"][0]["drawn"], json!(true), "{exported}");

    let (mut s, sheet, _, _, drawing) = holed();
    delete_hole(&mut s, &drawing);
    let exported = run(&mut s, "drawing.export", json!({"sheet": ids::encode(sheet)}));
    assert_eq!((&exported["dimensions"][0]["drawn"], &exported["dimensions"][0]["status"]), (&json!(false), &json!("lost")), "{exported}");
    assert!(exported["svg"].as_str().is_some_and(|svg| svg.contains("<svg")), "still written");
}

/// A saved view draws what it shows: with the second copy hidden, one cube's outline, 10 mm across
/// and 40 mm of line, where both copies are 40 mm across. The provider sees the change as it sees
/// any other setting's, and ends at the built scene; a view drawing nothing is left out and says why.
#[test]
fn a_view_draws_what_it_shows() {
    let (mut s, [first, second]) = two_cubes();
    run(&mut s, "drawing.create", json!({}));
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let made = run(&mut s, "drawing.view.base", json!({"sheet": ids::encode(sheet), "scale": "1:1", "position_mm": [150, 150]}));
    let view = ids::decode(made["view"].as_str().unwrap()).unwrap();
    let group = |scene: &Scene| scene.groups.iter().find(|g| g.object == view && g.kind == ObjectKind::View).cloned();
    let across = |scene: &Scene| {
        let (lo, hi, length) = reach(&group(scene).unwrap());
        (hi.x - lo.x, length)
    };

    let both = settled(&mut s, sheet);
    assert!((across(&both.scene).0 - 40.0).abs() < 1e-6, "{:?}", across(&both.scene));
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(view), "set": {"hide": [[second]]}}));
    let waiting = extract(&mut s, sheet).unwrap();
    assert_eq!((waiting.stale.as_slice(), bytes(&waiting.scene)), ([view].as_slice(), bytes(&both.scene)), "the last lines show until it is drawn");

    let one = settled(&mut s, sheet);
    let (wide, length) = across(&one.scene);
    assert!((wide - 10.0).abs() < 1e-6 && (length - 40.0).abs() < 1e-6, "{wide} {length}");
    assert_eq!(bytes(&one.scene), bytes(&built(&s, sheet).scene));
    let left = group(&one.scene).unwrap();
    let (lo, hi, _) = reach(&left);
    assert!((lo.x - 145.0).abs() < 1e-6 && (hi.x - 155.0).abs() < 1e-6, "its middle on its centre: {lo:?} {hi:?}");

    // The record allows a choice no occurrence is left in; a deleted one comes to the same.
    let mut d = (**s.drawing("test").unwrap()).clone();
    let at = d.views.iter().position(|v| v.id == view).unwrap();
    Arc::make_mut(&mut d.views[at]).settings.occurrences = Some(vec![vec![first]].into());
    Arc::make_mut(&mut d.views[at]).settings.hide = Some(vec![vec![first]].into());
    s.doc_mut().put_drawing(Arc::new(d));
    let scene = built(&s, sheet);
    assert_eq!(scene.views[0].status, Status::Placeholder(Unmeasured::NothingChosen));
    assert!(group(&scene.scene).is_none(), "left out, the sheet still draws");
    assert!(Unmeasured::NothingChosen.to_string().contains("may have been deleted"));
}

/// An object of a scene with `[min, max]` as its box: a view's boundary, or another object's bounds.
fn object_at(object: u64, kind: ObjectKind, min: [f64; 2], max: [f64; 2]) -> SheetObject {
    let b = Aabb2 { min: Vec2::from(min), max: Vec2::from(max) };
    let view = kind == ObjectKind::View;
    SheetObject { object, kind, bounds_mm: if view { Aabb2::EMPTY } else { b }, boundary_mm: view.then_some(b) }
}

/// The smallest box holding the point wins, then the topmost: nested, overlapping and equal boxes,
/// exactly on an edge, within a band and just past it. The sheet's border is never hit, and a point or
/// band that isn't a number hits nothing.
#[test]
fn a_point_hits_the_smallest_box_then_the_topmost() {
    let big = object_at(1, ObjectKind::View, [0.0, 0.0], [100.0, 100.0]);
    let small = object_at(2, ObjectKind::View, [40.0, 40.0], [60.0, 60.0]);
    let dimension = object_at(3, ObjectKind::Dimension, [45.0, 45.0], [55.0, 50.0]);
    let border = object_at(9, ObjectKind::DrawingBorder, [-10.0, -10.0], [500.0, 500.0]);
    let at = |x: f64, y: f64| Vec2::new(x, y);
    let id = |objects: &[SheetObject], p: Vec2, band: f64| hit(objects, p, band).map(|h| h.object);

    let nested = [border, big, small, dimension];
    assert_eq!(id(&nested, at(50.0, 47.0), 0.0), Some(3), "the dimension on the small view");
    assert_eq!(id(&nested, at(50.0, 55.0), 0.0), Some(2));
    assert_eq!(id(&nested, at(10.0, 10.0), 0.0), Some(1));
    assert_eq!(id(&nested, at(200.0, 200.0), 0.0), None, "the border is no object to pick");
    assert_eq!(hit(&nested, at(50.0, 47.0), 0.0).map(|h| h.kind), Some(ObjectKind::Dimension));
    // Whichever order they paint in, the smaller wins.
    assert_eq!(id(&[small, big], at(50.0, 50.0), 0.0), Some(2));
    assert_eq!(id(&[big, small], at(50.0, 50.0), 0.0), Some(2));
    // Two the same size: the one painted last.
    let twin = object_at(4, ObjectKind::View, [40.0, 40.0], [60.0, 60.0]);
    assert_eq!(id(&[small, twin], at(50.0, 50.0), 0.0), Some(4));
    assert_eq!(id(&[twin, small], at(50.0, 50.0), 0.0), Some(2));
    // A box only the band reaches still counts, and the smaller of the two wins.
    assert_eq!(id(&[big, small], at(39.0, 50.0), 2.0), Some(2));

    let on = |p: Vec2, band: f64| hit(&[small], p, band).map(|h| h.on_edge);
    assert_eq!(on(at(40.0, 50.0), 0.0), Some(true), "exactly on an edge, with no band");
    assert_eq!(on(at(60.0, 60.0), 0.0), Some(true), "a corner");
    assert_eq!(on(at(50.0, 50.0), 0.0), Some(false));
    assert_eq!(on(at(39.0, 50.0), 2.0), Some(true), "outside, within the band");
    assert_eq!(on(at(50.0, 62.0), 2.0), Some(true), "outside on the far side, at the band's end");
    assert_eq!(on(at(37.0, 50.0), 2.0), None);
    assert_eq!(on(at(42.0, 50.0), 2.0), Some(true), "inside, within the band");
    assert_eq!(on(at(43.0, 50.0), 2.0), Some(false));

    for (p, band) in [
        (at(f64::NAN, 50.0), 0.0),
        (at(50.0, f64::INFINITY), 0.0),
        (at(50.0, 50.0), f64::NAN),
        (at(50.0, 50.0), f64::INFINITY),
        (at(50.0, 50.0), -1.0),
    ] {
        assert_eq!(on(p, band), None, "{p:?} {band}");
    }
    assert_eq!(hit(&[], at(0.0, 0.0), 0.0), None);
}

/// The command answers from the sheet as drawn, evaluating nothing: a view, a dimension on it, and
/// nothing. A view that can't be drawn is still there to pick, and a band below 0, a band that isn't a
/// number and an unknown sheet are refused.
#[test]
fn hit_says_what_is_under_a_point_of_a_sheet() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let dimension = top_edge(&mut s, front);
    let sheet_id = ids::encode(sheet);
    let evaluations = s.evaluations;
    let ask =
        |s: &mut Session, point: [f64; 2], band: f64| run(s, "drawing.scene.hit", json!({"sheet": sheet_id, "point_mm": point, "band_mm": band}));
    let schema = output_schema_for::<crate::schema::drawing_scene::SheetHitResult>();

    let on_view = ask(&mut s, [150.0, 150.0], 0.0);
    assert!(conforms(schema.as_value(), schema.as_value(), &on_view), "{on_view}");
    assert_eq!(on_view["hit"], json!({"object": ids::encode(front), "kind": "view", "on_edge": false}));
    let on_dimension = ask(&mut s, [150.0, 165.0], 0.0);
    assert_eq!((&on_dimension["hit"]["object"], &on_dimension["hit"]["kind"]), (&json!(ids::encode(dimension)), &json!("dimension")));
    assert_eq!(ask(&mut s, [124.5, 150.0], 1.0)["hit"]["on_edge"], json!(true), "just outside the front view's left side, in the band");
    assert_eq!(ask(&mut s, [124.5, 150.0], 0.0)["hit"], Value::Null);
    let nothing = ask(&mut s, [10.0, 10.0], 0.0);
    assert!(conforms(schema.as_value(), schema.as_value(), &nothing), "{nothing}");
    assert_eq!(nothing["hit"], Value::Null);
    assert_eq!(s.evaluations, evaluations);

    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded_visible"}}));
    assert_eq!(ask(&mut s, [150.0, 220.0], 0.0)["hit"]["object"], json!(ids::encode(top)), "a view that can't be drawn");
    let default_band = run(&mut s, "drawing.scene.hit", json!({"sheet": sheet_id, "point_mm": [150.0, 150.0]}));
    assert_eq!(default_band["hit"]["object"], json!(ids::encode(front)));
    for band in [json!(-1.0), Value::Null, json!("2")] {
        let p = json!({"sheet": sheet_id, "point_mm": [150.0, 150.0], "band_mm": band});
        assert!(s.execute("drawing.scene.hit", &p).is_err(), "{band}");
    }
    assert!(s.execute("drawing.scene.hit", &json!({"sheet": "no such", "point_mm": [0.0, 0.0]})).is_err());
    assert!(s.execute("drawing.scene.hit", &json!({"sheet": sheet_id, "point_mm": [0.0]})).is_err());
}
