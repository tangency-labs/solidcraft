use std::collections::BTreeMap;

use serde_json::json;
use solidcraft_doc::drawing::view::{ViewKind, ViewPlace};

use super::*;
use crate::cmd::drawing_source::tests::{NO_PAIR, annotated, plates};
use crate::tests::Folder;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn refused(s: &mut Session, id: &str, p: Value) -> String {
    s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("{id} accepted: {p}"))
}

fn saved(s: &Session) -> String {
    s.drawing("test").unwrap().to_json().unwrap()
}

fn open_drawing(s: &Session) -> Arc<Drawing> {
    s.drawing("test").unwrap().clone()
}

fn id_of(v: &Value) -> u64 {
    ids::decode(v.as_str().unwrap()).unwrap()
}

/// What each pasted record was copied from, by the reply's `pasted` list: new id by old id.
fn copies(reply: &Value) -> BTreeMap<u64, u64> {
    reply["pasted"].as_array().unwrap().iter().map(|x| (id_of(&x["from"]), id_of(&x["id"]))).collect()
}

/// A plate 100 x 60 x 10 mm from the origin, called Plate, with a 10 mm hole through its middle,
/// made in the design itself; with `chamfer`, then a 2 mm chamfer on its top front edge.
fn plate(chamfer: bool) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 20, "base": [50, 30, -5], "operation": "cut", "targets": ["Plate"]}));
    if chamfer {
        run(&mut s, "solid.chamfer", json!({"edges": [[50, 0, 10]], "distance": 2, "body": "Plate"}));
    }
    s
}

/// A drawing of `s`'s design at `projection`'s angle, open, with a front view at 1:1 centred at
/// (150, 150) and the view projected to its right. Its first sheet and the two views.
fn front_and_right(s: &mut Session, projection: &str) -> (u64, u64, u64) {
    let sheet = run(s, "drawing.create", json!({"projection": projection}))["sheets"][0]["id"].clone();
    let front = run(s, "drawing.view.base", json!({"sheet": sheet, "scale": "1:1", "position_mm": [150, 150]}))["view"].clone();
    let right = run(s, "drawing.view.projected", json!({"parent": front, "side": "right"}))["view"].clone();
    (id_of(&sheet), id_of(&front), id_of(&right))
}

/// A length dimension on `view` of the edge through `point`.
fn length(s: &mut Session, view: u64, point: [f64; 3]) -> u64 {
    let kind = json!({"length": {"edge": {"point": point, "path": []}}});
    id_of(&run(s, "drawing.dimension.add", json!({"view": ids::encode(view), "kind": kind}))["dimension"])
}

/// The view and dimension groups `scene` draws, each object named by `rename` (ids it doesn't
/// have stay as they are), in paint order.
fn drawn(scene: &Value, rename: &BTreeMap<u64, u64>) -> Vec<Value> {
    let kinds = [json!("view"), json!("dimension")];
    let groups = scene["scene"]["groups"].as_array().unwrap().iter().filter(|g| kinds.contains(&g["kind"]));
    groups
        .map(|g| {
            let mut g = g.clone();
            let object = g["object"].as_u64().unwrap();
            g["object"] = json!(rename.get(&object).copied().unwrap_or(object));
            g
        })
        .collect()
}

/// The issue's case: a sheet with two views and five dimensions, pasted into another drawing of
/// the same design, draws the same views and dimensions, group by group, apart from the ids. The
/// drawing has a Sheet 1 already, so the pasted sheet is Sheet 2 (Add Sheet's rule); the right
/// view is still projected from the pasted front view; one undo step takes the paste out.
#[test]
fn a_sheet_pasted_into_another_drawing_draws_the_same() {
    let mut s = plate(false);
    let (sheet, front, right) = front_and_right(&mut s, "third");
    for point in [[50.0, 0.0, 0.0], [50.0, 0.0, 10.0], [0.0, 0.0, 5.0], [100.0, 0.0, 5.0]] {
        length(&mut s, front, point);
    }
    length(&mut s, right, [100.0, 30.0, 0.0]);
    let first = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    assert_eq!(drawn(&first, &BTreeMap::new()).len(), 7, "{first}");
    run(&mut s, COPY, json!({"sheets": [ids::encode(sheet)]}));
    run(&mut s, "drawing.create", json!({}));
    let before = saved(&s);
    let reply = run(&mut s, PASTE, json!({}));
    let made = copies(&reply);
    let kinds = [json!("sheet"), json!("view"), json!("dimension")];
    let counted = reply["pasted"].as_array().unwrap().iter().filter(|x| kinds.contains(&x["kind"])).count();
    assert_eq!(counted, 8, "the sheet, two views and five dimensions: {reply}");
    let d = open_drawing(&s);
    let pasted = d.sheets.iter().find(|x| x.id == made[&sheet]).unwrap();
    assert_eq!((pasted.name.as_str(), d.sheets.len()), ("Sheet 2", 2));
    assert!(matches!(d.view(made[&right]).unwrap().kind, ViewKind::Projected { parent, .. } if parent == made[&front]));
    let second = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(made[&sheet])}));
    assert_eq!(drawn(&first, &made), drawn(&second, &BTreeMap::new()));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(saved(&s), before, "one undo step");
}

/// From another design whose plate has no chamfer: the dimensions on edges both plates have come
/// across onto this design's plate, and the one on the chamfer's edge keeps its old record, is
/// listed as lost and isn't drawn.
#[test]
fn views_pasted_from_another_design_keep_the_dimensions_on_edges_it_has() {
    let mut source = plate(true);
    let (_, front, _) = front_and_right(&mut source, "third");
    let width = length(&mut source, front, [50.0, 0.0, 0.0]);
    let chamfer = length(&mut source, front, [50.0, 0.0, 8.0]);
    run(&mut source, COPY, json!({"views": [ids::encode(front)]}));
    let mut target = plate(false);
    run(&mut target, "drawing.create", json!({}));
    assert!(target.execute(PASTE, &json!({"from_design": true})).unwrap_err().to_string().contains("nothing was copied in another design"));
    target.foreign_copy = source.foreign_copy.clone();
    let reply = run(&mut target, PASTE, json!({"from_design": true}));
    let made = copies(&reply);
    assert_eq!(reply["kept"], json!([{"id": ids::encode(made[&width]), "kind": "dimension"}]), "{reply}");
    assert_eq!(reply["lost"], json!([{"id": ids::encode(made[&chamfer]), "kind": "dimension", "why": NO_PAIR}]), "{reply}");
    let read = run(&mut target, "drawing.dimension.inspect", json!({"dimension": ids::encode(made[&width])}));
    assert_eq!(read["status"], "ok", "{read}");
    assert!((read["value"].as_f64().unwrap() - 100.0).abs() < 1e-9, "{read}");
    let d = open_drawing(&target);
    let view = d.view(made[&front]).unwrap();
    assert!(matches!(view.kind, ViewKind::Base { source, .. } if source == d.sources[0].id), "this drawing's source");
    let sheet = ids::encode(view.sheet);
    let scene = run(&mut target, "drawing.scene", json!({"sheet": sheet}));
    let of = |id: u64| scene["dimensions"].as_array().unwrap().iter().find(|x| x["dimension"] == ids::encode(id)).unwrap()["drawn"].clone();
    assert_eq!((of(made[&width]), of(made[&chamfer])), (json!(true), json!(false)), "{scene}");
}

/// A sheet pasted into another design keeps its own border, title block and image: after the
/// design is saved and opened again, the image's bytes are there and the sheet draws all three.
/// Its documented component and its parts list become the design itself, and its revision tag
/// isn't copied: it is listed as left out.
#[test]
fn a_sheet_from_another_design_keeps_its_border_block_and_image_after_reopening() {
    let folder = Folder::new("drawing-paste-image");
    let mut source = plate(false);
    let sheet = run(&mut source, "drawing.create", json!({}))["sheets"][0]["id"].clone();
    let design = ids::encode(open_drawing(&source).sources[0].id);
    run(&mut source, "drawing.sheet.set", json!({"sheet": sheet, "documents": {"source": design}}));
    let file = folder.path("logo.png");
    std::fs::write(&file, crate::drawing_pictures::tests::png(4, 2)).unwrap();
    run(&mut source, "drawing.image.add", json!({"sheet": sheet, "file": file, "rect_mm": [20, 20, 60, 40]}));
    let list = run(&mut source, "drawing.table.add", json!({"sheet": sheet, "kind": "parts_list"}))["table"].clone();
    let note = run(&mut source, "drawing.note.add", json!({"anchor": {"sheet": sheet}, "at": [30, 30], "content": "General note"}))["note"].clone();
    let row = run(&mut source, "drawing.revision.add", json!({}))["row"].clone();
    let tag = run(&mut source, "drawing.revision.tag.add", json!({"row": row, "sheet": sheet, "at": [100, 50]}))["tag"].clone();
    run(&mut source, COPY, json!({"sheets": [sheet]}));
    let image = open_drawing(&source).images[0].clone();
    let mut target = plate(false);
    let drawing = id_of(&run(&mut target, "drawing.create", json!({}))["id"]);
    target.foreign_copy = source.foreign_copy.clone();
    let reply = run(&mut target, PASTE, json!({"from_design": true}));
    assert_eq!(reply["left_out"], json!([{"id": tag, "kind": "revision tag", "why": TAGS_STAY}]), "{reply}");
    let made = copies(&reply);
    let pasted = made[&id_of(&sheet)];
    let d = open_drawing(&target);
    let ours = solidcraft_doc::drawing::SheetComponent { source: d.sources[0].id, component: 0 };
    assert_eq!(d.sheets.iter().find(|x| x.id == pasted).unwrap().documents, Some(ours));
    let table = d.tables.by_id(made[&id_of(&list)]).unwrap();
    assert!(matches!(table.kind, solidcraft_doc::drawing::table::Kind::PartsList(l) if l.of == ours));
    assert!(d.revisions.tags().is_empty());
    assert_eq!(d.notes.by_id(made[&id_of(&note)]).unwrap().anchor, solidcraft_doc::drawing::note::Anchor::Sheet(pasted));
    let path = folder.path("target.solidcraft");
    run(&mut target, "file.save_as", json!({"path": path}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": path}));
    run(&mut opened, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let d = open_drawing(&opened);
    assert_eq!((d.images.len(), &d.images[0].data, &d.images[0].format), (1, &image.data, &image.format));
    let scene = run(&mut opened, "drawing.scene", json!({"sheet": ids::encode(pasted)}));
    let kinds: Vec<&Value> = scene["scene"]["groups"].as_array().unwrap().iter().map(|g| &g["kind"]).collect();
    for kind in ["drawing_border", "sheet_image", "title_block"] {
        assert!(kinds.contains(&&json!(kind)), "{kind}: {kinds:?}");
    }
}

/// Pasted twice, an image file is kept once: the second paste places the same file again.
#[test]
fn an_image_file_pasted_twice_is_kept_once() {
    let folder = Folder::new("drawing-paste-image-once");
    let mut s = plate(false);
    let sheet = run(&mut s, "drawing.create", json!({}))["sheets"][0]["id"].clone();
    let file = folder.path("logo.png");
    std::fs::write(&file, crate::drawing_pictures::tests::png(4, 2)).unwrap();
    run(&mut s, "drawing.image.add", json!({"sheet": sheet, "file": file, "rect_mm": [20, 20, 60, 40]}));
    run(&mut s, COPY, json!({"sheets": [sheet]}));
    run(&mut s, PASTE, json!({}));
    run(&mut s, PASTE, json!({}));
    let d = open_drawing(&s);
    assert_eq!((d.images.len(), d.sheet_images.len(), d.sheets.len()), (1, 3, 3));
    assert!(d.sheet_images.iter().all(|i| i.image == d.images[0].id));
}

/// A projected view pasted without its parent is the base view Copy View to Sheet makes of it:
/// the same direction, place and settings. The two keep one rule while Copy View to Sheet has its
/// own copier.
#[test]
fn a_projected_view_pasted_alone_is_the_view_copy_view_to_sheet_makes() {
    let mut s = plate(false);
    let (_, _, right) = front_and_right(&mut s, "third");
    let at = json!([300.0, 250.0]);
    let copied = run(&mut s, "drawing.view.copy_to_sheet", json!({"view": ids::encode(right), "sheet": "Sheet 1", "position_mm": at}));
    let copied = id_of(&copied["view"]);
    run(&mut s, COPY, json!({"views": [ids::encode(right)]}));
    let pasted = copies(&run(&mut s, PASTE, json!({"position_mm": at})))[&right];
    let d = open_drawing(&s);
    let (a, b) = (d.view(copied).unwrap(), d.view(pasted).unwrap());
    assert!(matches!(b.kind, ViewKind::Base { .. }));
    assert_eq!((&a.kind, &a.settings, a.place), (&b.kind, &b.settings, b.place));
}

/// Pasted views draw the design the drawing is in, and are drawn, even when the drawing lists a
/// source of another file first: its sources are kept by id, so the first can be any of them.
#[test]
fn pasted_views_draw_this_design_whatever_source_is_listed_first() {
    let mut s = plate(false);
    let (_, front, right) = front_and_right(&mut s, "third");
    run(&mut s, COPY, json!({"views": [ids::encode(front), ids::encode(right)]}));
    run(&mut s, "drawing.create", json!({}));
    let mut d = Drawing::clone(&open_drawing(&s));
    let own = d.own_source().unwrap();
    d.sources.insert(0, solidcraft_doc::drawing::Source { id: own.min(2) - 1, path: Some("Elsewhere/part.sc".into()) });
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    let made = copies(&run(&mut s, PASTE, json!({})));
    let d = open_drawing(&s);
    assert!(matches!(d.view(made[&front]).unwrap().kind, ViewKind::Base { source, .. } if source == own));
    let scene = run(&mut s, "drawing.scene", json!({"sheet": "Sheet 1"}));
    let statuses: Vec<&Value> = scene["views"].as_array().unwrap().iter().map(|v| &v["status"]).collect();
    assert_eq!(statuses, [&json!("drawn"), &json!("drawn")], "{scene}");
}

/// Into a drawing of the other projection angle, the projected view would show another side: it
/// is pasted as a base view that looks as it did, centred where it was, at its scale, and the
/// reply says how many views became base views.
#[test]
fn a_projected_view_pasted_at_the_other_angle_becomes_a_base_view_that_looks_the_same() {
    let mut s = plate(false);
    let (_, front, right) = front_and_right(&mut s, "third");
    let source = open_drawing(&s);
    let looked = source.resolve_view(right).unwrap();
    let centre = source.view_centre(right).unwrap();
    run(&mut s, COPY, json!({"views": [ids::encode(front), ids::encode(right)]}));
    run(&mut s, "drawing.create", json!({"projection": "first"}));
    let reply = run(&mut s, PASTE, json!({}));
    assert_eq!(reply["warnings"], json!([angles_differ(1)]), "{reply}");
    let made = copies(&reply);
    let d = open_drawing(&s);
    let view = d.view(made[&right]).unwrap();
    assert!(matches!(view.kind, ViewKind::Base { .. }) && matches!(view.place, ViewPlace::At(_)), "{view:?}");
    let now = d.resolve_view(made[&right]).unwrap();
    assert_eq!((now.basis, now.scale.value), (looked.basis, looked.scale.value));
    assert_eq!(d.view_centre(made[&right]).unwrap(), centre);
    assert_eq!(d.view_centre(made[&front]).unwrap(), source.view_centre(front).unwrap());
}

/// `position_mm` puts the first view's centre there and keeps the others' spacing; a place off
/// the sheet's paper is refused with nothing changed, and so is a copy of views on two sheets.
#[test]
fn pasted_views_keep_their_spacing_and_stay_on_the_paper() {
    let mut s = plate(false);
    let (_, front, right) = front_and_right(&mut s, "third");
    let d = open_drawing(&s);
    let copied = run(&mut s, COPY, json!({"views": [ids::encode(front), ids::encode(right)]}));
    // `position_mm` is for the first view the copy lists; the other keeps its offset from it.
    let first = copied["views"][0].as_str().and_then(ids::decode).unwrap();
    let second = if first == front { right } else { front };
    let gap = d.view_centre(second).unwrap() - d.view_centre(first).unwrap();
    let before = saved(&s);
    let why = refused(&mut s, PASTE, json!({"position_mm": [-500, 100]}));
    assert!(why.contains("off the paper"), "{why}");
    assert_eq!(saved(&s), before);
    let made = copies(&run(&mut s, PASTE, json!({"position_mm": [100, 120]})));
    let d = open_drawing(&s);
    let at = d.view_centre(made[&first]).unwrap();
    assert_eq!((at.x, at.y), (100.0, 120.0));
    assert_eq!(d.view_centre(made[&second]).unwrap() - at, gap);
    let lone = run(&mut s, COPY, json!({"views": [ids::encode(right)]}));
    assert_eq!(lone["views"], json!([ids::encode(right)]), "{lone}");
    let reply = run(&mut s, PASTE, json!({}));
    let alone = open_drawing(&s).view(copies(&reply)[&right]).unwrap().clone();
    assert!(matches!(alone.kind, ViewKind::Base { .. }) && reply["warnings"] == json!([]), "{reply}");
    let other = run(&mut s, "drawing.sheet.add", json!({}))["sheet"].clone();
    let elsewhere = run(&mut s, "drawing.view.base", json!({"sheet": other, "scale": "1:1"}))["view"].clone();
    let why = refused(&mut s, COPY, json!({"views": [ids::encode(front), elsewhere]}));
    assert!(why.contains("one sheet"), "{why}");
}

/// The copy is a snapshot: a dimension deleted after the copy is still pasted. Two pastes make
/// two copies with ids of their own, and undo takes out only the second.
#[test]
fn the_copy_is_a_snapshot_and_each_paste_one_undo_step() {
    let mut s = plate(false);
    let (_, front, _) = front_and_right(&mut s, "third");
    let width = length(&mut s, front, [50.0, 0.0, 0.0]);
    run(&mut s, COPY, json!({"views": [ids::encode(front)]}));
    s.edit_drawing("test", |d| {
        d.dimensions.retain(|x| x.id != width);
        Ok(())
    })
    .unwrap();
    let one = copies(&run(&mut s, PASTE, json!({})));
    let after_one = saved(&s);
    let two = copies(&run(&mut s, PASTE, json!({})));
    assert!(open_drawing(&s).dimension(one[&width]).is_some() && open_drawing(&s).dimension(two[&width]).is_some());
    assert!(one.values().all(|id| !two.values().any(|other| other == id)), "{one:?} {two:?}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(saved(&s), after_one);
}

/// In the same design, `to` makes the pasted views show another part, as a replace does: the
/// annotations that still find their one thing move onto it, and the chamfer's dimension is lost.
/// The pasted sheet documents that part, its parts list lists it, and the balloon reads the pasted
/// list.
#[test]
fn a_sheet_pasted_with_to_shows_that_part() {
    let mut p = plates();
    let [width, note, balloon, chamfer] = annotated(&mut p);
    let d = open_drawing(&p.s);
    let (sheet, design) = (ids::encode(d.sheets[0].id), ids::encode(d.sources[0].id));
    run(&mut p.s, "drawing.sheet.set", json!({"sheet": sheet, "documents": {"source": design, "component": p.a}}));
    let list = run(&mut p.s, "drawing.table.add", json!({"sheet": sheet, "kind": "parts_list"}))["table"].clone();
    p.s.edit_drawing("test", |d| {
        let at = d.balloons.index_of(id_of(&balloon)).unwrap();
        Arc::make_mut(&mut d.balloons[at]).parts_list = Some(id_of(&list));
        Ok(())
    })
    .unwrap();
    run(&mut p.s, COPY, json!({"sheets": [sheet]}));
    let reply = run(&mut p.s, PASTE, json!({"to": {"component": p.b}}));
    let made = copies(&reply);
    let new = |v: &Value| json!(ids::encode(made[&id_of(v)]));
    let kept = json!([{"id": new(&width), "kind": "dimension"}, {"id": new(&note), "kind": "note"}, {"id": new(&balloon), "kind": "balloon"}]);
    assert_eq!(reply["kept"], kept, "{reply}");
    assert_eq!(reply["lost"], json!([{"id": new(&chamfer), "kind": "dimension", "why": NO_PAIR}]), "{reply}");
    let d = open_drawing(&p.s);
    let view = d.view(made[&p.view]).unwrap();
    assert_eq!(view.settings.occurrences.as_deref(), Some(&[vec![p.b_at]][..]));
    assert_eq!(d.view(p.view).unwrap().settings.occurrences.as_deref(), Some(&[vec![p.a_at]][..]), "the original is unchanged");
    let b = solidcraft_doc::drawing::SheetComponent { source: d.sources[0].id, component: p.b };
    assert_eq!(d.sheets.iter().find(|x| x.id == made[&d.sheets[0].id]).unwrap().documents, Some(b));
    let pasted = made[&id_of(&list)];
    assert!(matches!(d.tables.by_id(pasted).unwrap().kind, solidcraft_doc::drawing::table::Kind::PartsList(l) if l.of == b));
    assert_eq!(d.balloons.by_id(made[&id_of(&balloon)]).unwrap().parts_list, Some(pasted));
}

/// With `to`, every pasted sheet that documents a component, and every pasted parts list, documents
/// and lists that part, whichever part it named before: here the design itself, not the part its
/// views showed.
#[test]
fn with_to_every_pasted_sheet_and_parts_list_follow() {
    let mut p = plates();
    let d = open_drawing(&p.s);
    let (old, design) = (d.sheets[0].id, ids::encode(d.sources[0].id));
    let sheet = ids::encode(old);
    run(&mut p.s, "drawing.sheet.set", json!({"sheet": sheet, "documents": {"source": design, "component": 0}}));
    let list = run(&mut p.s, "drawing.table.add", json!({"sheet": sheet, "kind": "parts_list"}))["table"].clone();
    run(&mut p.s, COPY, json!({"sheets": [sheet]}));
    let made = copies(&run(&mut p.s, PASTE, json!({"to": {"component": p.b}})));
    let d = open_drawing(&p.s);
    let b = solidcraft_doc::drawing::SheetComponent { source: d.sources[0].id, component: p.b };
    assert_eq!(d.sheets.iter().find(|x| x.id == made[&old]).unwrap().documents, Some(b));
    assert!(matches!(d.tables.by_id(made[&id_of(&list)]).unwrap().kind, solidcraft_doc::drawing::table::Kind::PartsList(l) if l.of == b));
}

/// From another design, a view's hide paths name that design's occurrences, so every one is
/// dropped and listed, even one outside the part it shows.
#[test]
fn views_from_another_design_drop_every_hide_path() {
    let mut p = plates();
    let b_at = p.b_at;
    p.s.edit_drawing("test", |d| {
        Arc::make_mut(&mut d.views[0]).settings.hide = Some(Arc::from(vec![vec![b_at]]));
        Ok(())
    })
    .unwrap();
    run(&mut p.s, COPY, json!({"views": [ids::encode(p.view)]}));
    let mut target = plate(false);
    run(&mut target, "drawing.create", json!({}));
    target.foreign_copy = p.s.foreign_copy.clone();
    let reply = run(&mut target, PASTE, json!({"from_design": true}));
    let view = ids::encode(copies(&reply)[&p.view]);
    assert_eq!(reply["dropped_hide"], json!([{"view": view, "path": [b_at]}]), "{reply}");
    assert!(open_drawing(&target).views[0].settings.hide.is_none());
}

/// A table pasted alone goes on the sheet named, keeping its place there.
#[test]
fn a_table_pasted_alone_goes_on_the_sheet_named() {
    let mut s = plate(false);
    let sheet = run(&mut s, "drawing.create", json!({}))["sheets"][0]["id"].clone();
    let table = run(&mut s, "drawing.table.add", json!({"sheet": sheet, "kind": "revision"}))["table"].clone();
    let other = run(&mut s, "drawing.sheet.add", json!({}))["sheet"].clone();
    run(&mut s, COPY, json!({"tables": [table]}));
    let made = copies(&run(&mut s, PASTE, json!({"sheet": other})));
    let d = open_drawing(&s);
    let (original, pasted) = (d.tables.by_id(id_of(&table)).unwrap(), d.tables.by_id(made[&id_of(&table)]).unwrap());
    assert_eq!((pasted.sheet, &pasted.kind, &pasted.place), (id_of(&other), &original.kind, &original.place));
}

/// Refused with nothing changed: nothing copied, not exactly one of views, sheets and tables, an
/// id the drawing doesn't have, sheets with a place, an unknown `to`.
#[test]
fn bad_copies_and_pastes_are_refused() {
    let mut s = plate(false);
    let (sheet, front, _) = front_and_right(&mut s, "third");
    assert!(refused(&mut s, PASTE, json!({})).contains("nothing copied"));
    for p in [json!({}), json!({"views": []}), json!({"views": [ids::encode(front)], "sheets": [ids::encode(sheet)]}), json!({"views": ["zz"]})] {
        refused(&mut s, COPY, p);
    }
    let before = saved(&s);
    run(&mut s, COPY, json!({"sheets": [ids::encode(sheet)]}));
    assert!(refused(&mut s, PASTE, json!({"position_mm": [10, 10]})).contains("whole"));
    run(&mut s, COPY, json!({"views": [ids::encode(front)]}));
    assert!(refused(&mut s, PASTE, json!({"to": {"component": 99}})).contains("99"));
    assert_eq!(saved(&s), before);
}

/// Every hostile value in every place of the paste's and the copy's parameters is answered without
/// a panic, and every drawing still keeps its rules.
#[test]
fn hostile_clipboard_params_never_panic() {
    let mut p = plates();
    annotated(&mut p);
    let sheet = ids::encode(open_drawing(&p.s).sheets[0].id);
    let table = run(&mut p.s, "drawing.table.add", json!({"sheet": sheet, "kind": "revision"}))["table"].clone();
    let copies = [json!({"views": [ids::encode(p.view)]}), json!({"sheets": [sheet]}), json!({"tables": [table]})];
    let paste = json!({"sheet": sheet, "position_mm": [10, 10], "to": {"component": p.b, "path": [p.b_at]}, "from_design": false});
    run(&mut p.s, COPY, json!({"views": [ids::encode(p.view)]}));
    let mut values = crate::tests::hostile_values();
    values.extend([json!(u64::MAX), Value::Array(vec![json!(1); 1000]), json!("x".repeat(600)), json!({"component": 0})]);
    let mut internal = Vec::new();
    for (id, template) in copies.iter().map(|copy| (COPY, copy)).chain([(PASTE, &paste)]) {
        let mut at = Vec::new();
        crate::tests::json_places(template, String::new(), &mut at);
        for (place, value) in at.iter().flat_map(|place| values.iter().map(move |value| (place, value))) {
            let mut params = template.clone();
            if let Some(slot) = params.pointer_mut(place) {
                *slot = value.clone();
            }
            let mut scratch = p.s.scratch();
            if let Err(crate::EngineError::Internal(_, msg)) = scratch.execute(id, &params) {
                internal.push(format!("{id} {params}: {msg}"));
            }
            scratch.doc.drawings.iter().for_each(|d| d.validate().unwrap());
        }
    }
    assert!(internal.is_empty(), "the clipboard panicked:\n{}", internal.join("\n"));
}
