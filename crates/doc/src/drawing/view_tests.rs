use solidcraft_geom::prelude::*;
use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::cell::Projection;
use solidcraft_geom::drawing_sheet::{self, Orientation, PaperSize};
use solidcraft_geom::drawing_view::NamedView;
use solidcraft_geom::paper::line::LineGroup;

use super::*;
use crate::drawing::{Excluded, Sheet, Source};
use crate::drawing_view::{Exploded, JointPose, OccurrencePaths, Pose, StudyStep};
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

/// A view on the drawing's first sheet, showing no more for its reference parts.
fn add(d: &mut Drawing, id_source: &mut IdSource, kind: ViewKind, place: ViewPlace, settings: ViewOverrides) -> u64 {
    let id = d.allocate_id(id_source).unwrap();
    let sheet = d.sheets[0].id;
    d.add_views([Arc::new(View { id, sheet, kind, place, settings, reference_shown_percent: 0.0 })]);
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
    ViewOverrides { visual_style: Some(style), ..ViewOverrides::default() }
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
            {"id": right_id, "sheet": sheet, "kind": {"projected": {"parent": front_id, "side": "right"}}, "place": {"aligned": 320.0}, "settings": {"visual_style": "visible_hidden"}},
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

/// A view takes the views projected from it, through others and whichever way their ids sort, and
/// nothing above it or beside it; none given, none taken.
#[test]
fn a_views_projected_views_are_found_down_its_chains() {
    let (mut d, mut id_source, [front, right, top]) = three();
    let left = add(&mut d, &mut id_source, projected(front, ProjectedSide::Left), ViewPlace::Aligned(20.0), ViewOverrides::default());
    let set = |ids: &[u64]| ids.iter().copied().collect::<BTreeSet<u64>>();
    assert_eq!(d.with_projected(&set(&[front])), set(&[front, right, top, left]));
    assert_eq!(d.with_projected(&set(&[right])), set(&[right, top]));
    assert_eq!(d.with_projected(&set(&[top, left])), set(&[top, left]));
    assert_eq!(d.with_projected(&set(&[])), set(&[]));
    assert_eq!(d.with_projected(&set(&[99])), set(&[99]));
}

/// What a view shows saves in its settings, follows down its chain until a view sets its own, and
/// reads back as the same bytes; a view that sets neither saves neither.
#[test]
fn what_a_view_shows_follows_its_parent_and_saves_as_it_is() {
    let (mut d, _, [front, right, top]) = three();
    let paths = |ids: &[&[u64]]| -> OccurrencePaths { ids.iter().map(|path| path.to_vec()).collect() };
    let resolved = |d: &Drawing, id: u64| d.resolve_view(id).unwrap();
    let none = resolved(&d, top);
    assert_eq!((none.occurrences.value, none.occurrences.set_by, none.hide.value, none.hide.set_by), (None, SetBy::Default, None, SetBy::Default));
    view_mut(&mut d, front).settings.occurrences = Some(paths(&[&[7], &[]]));
    view_mut(&mut d, front).settings.hide = Some(paths(&[&[7, 8]]));
    d.validate().unwrap();
    let followed = resolved(&d, top);
    assert_eq!((followed.occurrences.value, followed.occurrences.set_by), (Some(paths(&[&[7], &[]])), SetBy::Parent(front)));
    assert_eq!((followed.hide.value, followed.hide.set_by), (Some(paths(&[&[7, 8]])), SetBy::Parent(front)));
    // An empty list is a setting of its own: hiding nothing, whatever the parent hides.
    view_mut(&mut d, right).settings.hide = Some(paths(&[]));
    let own = resolved(&d, top);
    assert_eq!((own.hide.value, own.hide.set_by), (Some(paths(&[])), SetBy::Parent(right)));
    assert_eq!(own.occurrences.set_by, SetBy::Parent(front), "each setting on its own");
    let v = file(&d);
    let listed = v["views"].as_array().unwrap();
    let saved = |id: u64| listed.iter().find(|view| view["id"] == json!(ids::encode(id))).unwrap();
    assert_eq!(saved(front)["settings"], json!({"scale": "1:2", "occurrences": [[7], []], "hide": [[7, 8]]}));
    assert_eq!(saved(right)["settings"], json!({"visual_style": "visible_hidden", "hide": []}));
    assert_eq!(saved(top).get("settings"), None);
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), d.to_json().unwrap());
}

/// The choice's limits are kept on reading, at each side of each limit. A path no occurrence makes
/// reads: the record doesn't check it against the design.
#[test]
fn what_a_view_shows_keeps_its_limits_on_reading() {
    let (d, _, _) = three();
    let at = d.views.iter().position(|view| matches!(view.kind, ViewKind::Base { .. })).unwrap();
    let with = |key: &str, paths: Value| {
        refused(&d, |v| {
            v["views"][at]["settings"][key] = paths;
        })
    };
    let lists = |paths: usize, ids: usize| json!(vec![vec![1u64; ids]; paths]);
    // One id past the cap: 64 paths of 64, then one of 1.
    let one_past = json!([vec![vec![1u64; 64]; 64], vec![vec![1u64]]].concat());
    let cases = [
        ("occurrences", json!([[999_999]]), "read"),
        ("occurrences", json!([[]]), "read"),
        ("occurrences", json!([]), "read"),
        ("hide", json!([]), "read"),
        ("hide", lists(1000, 1), "read"),
        ("hide", lists(1001, 1), "at most 1000 occurrence paths"),
        ("occurrences", lists(1000, 1), "read"),
        ("occurrences", lists(1001, 0), "at most 1000 occurrence paths"),
        ("hide", lists(64, 64), "read"),
        ("hide", lists(65, 64), "4096 ids in all"),
        ("hide", one_past, "4096 ids in all"),
        ("occurrences", lists(1, 64), "read"),
        ("occurrences", lists(1, 65), "1..64 ids"),
        ("hide", lists(1, 65), "1..64 ids"),
        ("hide", json!([[]]), "hide []"),
        ("hide", json!([[1], []]), "hide []"),
        ("hide", json!(null), "read"),
        ("hide", json!([[-1]]), "invalid value"),
        ("occurrences", json!([[1.5]]), "invalid"),
        ("occurrences", json!(["a"]), "invalid"),
        ("occurrences", json!(7), "invalid"),
    ];
    for (key, paths, says) in cases {
        let e = with(key, paths.clone());
        assert!(e.contains(says), "{key} {}: {e}", paths.to_string().chars().take(60).collect::<String>());
    }
    // The ids of both lists count together.
    let both = refused(&d, |v| {
        v["views"][at]["settings"]["occurrences"] = lists(40, 64);
        v["views"][at]["settings"]["hide"] = lists(25, 64);
    });
    assert!(both.contains("4096 ids in all"), "{both}");
}

/// Which exploded view a view draws saves in its settings by the exploded view's id, follows down its
/// chain until a view sets its own (`assembled` included), and reads back as the same bytes; a view
/// that sets none saves none and draws assembled.
#[test]
fn an_exploded_choice_follows_its_parent_and_saves_as_it_is() {
    let (mut d, _, [front, right, top]) = three();
    let resolved = |d: &Drawing, id: u64| d.resolve_view(id).unwrap().exploded;
    assert_eq!(resolved(&d, top), Setting { value: Exploded::Assembled, set_by: SetBy::Default });
    view_mut(&mut d, front).settings.exploded = Some(Exploded::View(77));
    d.validate().unwrap();
    assert_eq!(resolved(&d, top), Setting { value: Exploded::View(77), set_by: SetBy::Parent(front) });
    assert_eq!(resolved(&d, front), Setting { value: Exploded::View(77), set_by: SetBy::This });
    // Assembled is a setting of its own: the views below it draw assembled whatever the base draws.
    view_mut(&mut d, right).settings.exploded = Some(Exploded::Assembled);
    assert_eq!(resolved(&d, top), Setting { value: Exploded::Assembled, set_by: SetBy::Parent(right) });
    let v = file(&d);
    let listed = v["views"].as_array().unwrap();
    let saved = |id: u64| listed.iter().find(|view| view["id"] == json!(ids::encode(id))).unwrap();
    // The design's ids, as occurrence paths save them: numbers.
    assert_eq!(saved(front)["settings"], json!({"scale": "1:2", "exploded": {"view": 77}}));
    assert_eq!(saved(right)["settings"], json!({"visual_style": "visible_hidden", "exploded": "assembled"}));
    assert_eq!(saved(top).get("settings"), None);
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), d.to_json().unwrap());
}

/// An exploded choice reads whatever exploded view it names (one the design no longer has draws a
/// placeholder, never refuses the file), and anything that isn't a choice is refused.
#[test]
fn an_exploded_choice_keeps_its_limits_on_reading() {
    let (d, _, _) = three();
    let at = d.views.iter().position(|view| matches!(view.kind, ViewKind::Base { .. })).unwrap();
    let with = |choice: Value| {
        refused(&d, |v| {
            v["views"][at]["settings"]["exploded"] = choice;
        })
    };
    // What each reads as: "read", or a part of why it is refused ("" for refused, whatever the words).
    let cases = [
        (json!({"view": 999_999}), "read"),
        (json!({"view": u64::MAX}), "read"),
        (json!({"view": 0}), "read"),
        (json!("assembled"), "read"),
        (json!(null), "read"),
        (json!("exploded"), "unknown variant"),
        (json!({"view": "7"}), "invalid type"),
        (json!({"view": -1}), "invalid value"),
        (json!({"view": 1.5}), "invalid type"),
        (json!({"view": 7, "amount": 1}), ""),
        (json!({"view": null}), ""),
        (json!([]), ""),
        (json!(1.5), ""),
    ];
    for (choice, says) in cases {
        let e = with(choice.clone());
        assert!(if says.is_empty() { e != "read" } else { e.contains(says) }, "{choice}: {e}");
    }
}

/// A view's position saves in its settings as its joints' values, by the joints' ids, follows down its
/// chain until a view sets its own (`designed` included), and reads back as the same bytes; a view
/// that sets none saves none and draws the design as it places its parts.
#[test]
fn a_pose_follows_its_parent_and_saves_as_it_is() {
    let (mut d, _, [front, right, top]) = three();
    let resolved = |d: &Drawing, id: u64| d.resolve_view(id).unwrap().pose;
    assert_eq!(resolved(&d, top), Setting { value: Pose::Designed, set_by: SetBy::Default });
    let joints = [JointPose { joint: 77, values: vec![std::f64::consts::FRAC_PI_2] }, JointPose { joint: 78, values: vec![0.5, -12.0] }];
    let opened = Pose::Joints(Arc::from(joints));
    view_mut(&mut d, front).settings.pose = Some(opened.clone());
    d.validate().unwrap();
    assert_eq!(resolved(&d, top), Setting { value: opened.clone(), set_by: SetBy::Parent(front) });
    assert_eq!(resolved(&d, front), Setting { value: opened, set_by: SetBy::This });
    // Designed is a setting of its own: the views below it draw the design's placement whatever the base draws.
    view_mut(&mut d, right).settings.pose = Some(Pose::Designed);
    assert_eq!(resolved(&d, top), Setting { value: Pose::Designed, set_by: SetBy::Parent(right) });
    let v = file(&d);
    let listed = v["views"].as_array().unwrap();
    let saved = |id: u64| listed.iter().find(|view| view["id"] == json!(ids::encode(id))).unwrap();
    // The design's ids, as an exploded choice saves them: numbers; the values in radians and mm.
    let pose = json!({"joints": [{"joint": 77, "values": [std::f64::consts::FRAC_PI_2]}, {"joint": 78, "values": [0.5, -12.0]}]});
    assert_eq!(saved(front)["settings"], json!({"scale": "1:2", "pose": pose}));
    assert_eq!(saved(right)["settings"], json!({"visual_style": "visible_hidden", "pose": "designed"}));
    assert_eq!(saved(top).get("settings"), None);
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), d.to_json().unwrap());
}

/// A view's position at a motion study's step saves as the study's id and the step, follows down its
/// chain as joint values do until a view sets its own, and reads back as the same bytes.
#[test]
fn a_study_step_follows_its_parent_and_saves_as_it_is() {
    let (mut d, _, [front, right, top]) = three();
    let resolved = |d: &Drawing, id: u64| d.resolve_view(id).unwrap().pose;
    let at = Pose::Study(StudyStep { id: 31, step: 40 });
    view_mut(&mut d, front).settings.pose = Some(at.clone());
    d.validate().unwrap();
    assert_eq!(resolved(&d, top), Setting { value: at.clone(), set_by: SetBy::Parent(front) });
    view_mut(&mut d, right).settings.pose = Some(Pose::Designed);
    assert_eq!(resolved(&d, top), Setting { value: Pose::Designed, set_by: SetBy::Parent(right) });
    let v = file(&d);
    let saved = v["views"].as_array().unwrap().iter().find(|view| view["id"] == json!(ids::encode(front))).cloned().unwrap();
    assert_eq!(saved["settings"], json!({"scale": "1:2", "pose": {"study": {"id": 31, "step": 40}}}));
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), d.to_json().unwrap());
}

/// A position's limits are kept on reading, at each side of each limit. A joint the design no longer
/// has reads (the view skips it, never refuses the file), and anything that isn't a position is refused.
#[test]
fn a_pose_keeps_its_limits_on_reading() {
    let (d, _, _) = three();
    let at = d.views.iter().position(|view| matches!(view.kind, ViewKind::Base { .. })).unwrap();
    let with = |pose: Value| {
        refused(&d, |v| {
            v["views"][at]["settings"]["pose"] = pose;
        })
    };
    // Joints 0.. of a position, each with `values` values.
    let joints = |count: u64, values: usize| {
        let listed: Vec<Value> = (0..count).map(|joint| json!({"joint": joint, "values": vec![0.25; values]})).collect();
        json!({"joints": listed})
    };
    let one = |values: Value| json!({"joints": [{"joint": 7, "values": values}]});
    // What each reads as: "read", or a part of why it is refused ("" for refused, whatever the words).
    let cases = [
        (joints(64, 6), "read"),
        (joints(65, 1), "at most 64 joints"),
        (joints(1, 7), "1..6 values"),
        (joints(1, 0), "1..6 values"),
        (json!({"joints": []}), "at least one joint"),
        (one(json!([1e9, -1e9])), "read"),
        // A float past 1e9 is the file's own number check, before the position's.
        (one(json!([1.000001e9])), "is out of range"),
        // A whole number passes the file's check, so the position refuses it.
        (one(json!([1_000_001_000_u64])), "at most 1000000000"),
        (one(json!([-1_000_001_000_i64])), "at most 1000000000"),
        (json!({"joints": [{"joint": 999_999, "values": [1]}]}), "read"),
        (json!({"joints": [{"joint": 7, "values": [1]}, {"joint": 7, "values": [2]}]}), "each joint once"),
        (json!("designed"), "read"),
        (json!(null), "read"),
        (json!({"joints": [{"joint": -1, "values": [1]}]}), "invalid value"),
        (json!({"joints": [{"joint": "7", "values": [1]}]}), "invalid type"),
        (one(json!(["1"])), "invalid type"),
        (json!({"joints": [{"joint": 7, "values": [1], "limit": 2}]}), "unknown field"),
        (json!({"joints": [{"joint": 7}]}), "missing field"),
        (json!("posed"), "unknown variant"),
        (json!([]), ""),
        (json!(1.5), ""),
        // A study step: a study the design no longer has reads (the view draws the design's placement).
        (json!({"study": {"id": 999_999, "step": 0}}), "read"),
        (json!({"study": {"id": 3, "step": u32::MAX}}), "read"),
        (json!({"study": {"id": 3, "step": u64::from(u32::MAX) + 1}}), "invalid value"),
        (json!({"study": {"id": 3, "step": -1}}), "invalid value"),
        (json!({"study": {"id": 3, "step": 2.5}}), "invalid type"),
        (json!({"study": {"id": -1, "step": 0}}), "invalid value"),
        (json!({"study": {"step": 0}}), "missing field"),
        (json!({"study": {"id": 3}}), "missing field"),
        (json!({"study": {"id": 3, "step": 0, "at": 1}}), "unknown field"),
        (json!({"study": 3}), "invalid type"),
    ];
    for (pose, says) in cases {
        let e = with(pose.clone());
        assert!(if says.is_empty() { e != "read" } else { e.contains(says) }, "{}: {e}", pose.to_string().chars().take(60).collect::<String>());
    }
}

/// Seeded changes to a view's saved choice: each reads or is refused, never panics, and what reads
/// saves as the same bytes and resolves.
#[test]
fn seeded_changes_to_what_a_view_shows_read_or_are_refused() {
    let (d, _, ids_of) = three();
    let pick = |seed: u64, salt: u64, n: u64| ((seed ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) % n;
    let id = |seed: u64, salt: u64| -> Value {
        [json!(1), json!(0), json!(u64::MAX), json!(-1), json!(1.5), json!("1"), json!(null), json!([1]), json!(f64::MAX)]
            [pick(seed, salt, 9) as usize]
            .clone()
    };
    let path = |seed: u64, salt: u64| -> Value {
        match pick(seed, salt, 6) {
            0 => json!([]),
            1 => json!(vec![1u64; 64]),
            2 => json!(vec![1u64; 65]),
            3 => id(seed, salt + 1),
            _ => json!((0..pick(seed, salt + 2, 4)).map(|i| id(seed, salt + 3 + i)).collect::<Vec<_>>()),
        }
    };
    for seed in 1..=500u64 {
        let list = |salt: u64| -> Value {
            match pick(seed, salt, 5) {
                0 => json!(null),
                1 => json!({"occurrences": []}),
                _ => json!((0..pick(seed, salt + 1, 4)).map(|i| path(seed, salt + 10 + i * 10)).collect::<Vec<_>>()),
            }
        };
        let at = pick(seed, 1, 3) as usize;
        let mut v = file(&d);
        v["views"][at]["settings"]["occurrences"] = list(100);
        v["views"][at]["settings"]["hide"] = list(200);
        let Ok(read) = Drawing::from_json(&v.to_string()) else { continue };
        let again = read.to_json().unwrap();
        assert_eq!(Drawing::from_json(&again).unwrap().to_json().unwrap(), again, "seed {seed}");
        for view in ids_of {
            read.resolve_view(view).unwrap();
        }
    }
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
        line_group: None,
    }));
    let other_id = ids::encode(other);
    assert_eq!(
        refused(&d, |v| v["views"][2]["sheet"] = json!(other_id)),
        rule(&right_id, "a view lined up with its parent is on its parent's sheet")
    );
    assert_eq!(refused(&d, |v| v["views"][1]["sheet"] = json!(other_id)), "read");
    // Its place and its reference_shown_percent.
    let off_paper = "its place is at most 5000 mm from the sheet's origin";
    assert_eq!(refused(&d, |v| v["views"][1]["place"] = json!({"at": [5000.0, -5000.0]})), "read");
    assert_eq!(refused(&d, |v| v["views"][1]["place"] = json!({"at": [5000.001, 0.0]})), rule(&top_id, off_paper));
    assert_eq!(refused(&d, |v| v["views"][2]["place"] = json!({"aligned": -5000.001})), rule(&right_id, off_paper));
    for (percent, read) in [(0.0, true), (100.0, true), (-0.001, false), (100.001, false)] {
        let why = refused(&d, |v| v["views"][1]["reference_shown_percent"] = json!(percent));
        let expected = if read { "read".to_string() } else { rule(&top_id, "its reference_shown_percent is 0 to 100") };
        assert_eq!(why, expected, "{percent}");
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
/// it, and its own reference_shown_percent.
#[test]
fn a_view_resolves_through_its_chain() {
    let (mut d, _, [front, right, top]) = three();
    let view = view_mut(&mut d, top);
    view.settings.tangent_edges = Some(TangentEdges::Show);
    view.reference_shown_percent = 2.0;
    let resolved = d.resolve_view(top).unwrap();
    assert_eq!(resolved.source, d.sources[0].id);
    let right_basis = projected_basis(&NamedView::Front.basis(), ProjectedSide::Right, Projection::Third).unwrap();
    assert_eq!(resolved.basis, projected_basis(&right_basis, ProjectedSide::Above, Projection::Third).unwrap());
    assert_eq!(resolved.scale, Setting { value: "1:2".parse().unwrap(), set_by: SetBy::Parent(front) });
    assert_eq!(resolved.visual_style, Setting { value: VisualStyle::VisibleHidden, set_by: SetBy::Parent(right) });
    assert_eq!(resolved.tangent_edges, Setting { value: TangentEdges::Show, set_by: SetBy::This });
    assert_eq!(resolved.reference_style, Setting { value: VisualStyle::Visible, set_by: SetBy::Default });
    assert_eq!(resolved.reference_hides_model, Setting { value: false, set_by: SetBy::Default });
    assert_eq!(resolved.reference_shown_percent, 2.0);
    // The base view gives its own; its projected views follow the drawing's projection.
    let base = d.resolve_view(front).unwrap();
    assert_eq!((base.basis, base.scale.set_by, base.visual_style.set_by), (NamedView::Front.basis(), SetBy::This, SetBy::Default));
    assert_eq!(d.resolve_view(right).unwrap().basis, NamedView::Right.basis());
    d.projection = Projection::First;
    assert_eq!(d.resolve_view(right).unwrap().basis, NamedView::Left.basis());
    // A view's own setting wins over its parent's.
    view_mut(&mut d, top).settings.visual_style = Some(VisualStyle::Visible);
    assert_eq!(d.resolve_view(top).unwrap().visual_style, Setting { value: VisualStyle::Visible, set_by: SetBy::This });
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

/// A view no view of its chain sets a line group for draws in its sheet's, else its drawing's,
/// and says which gave it.
#[test]
fn a_view_draws_in_its_sheets_line_group_else_the_drawings() {
    let (mut d, _, [front, right, _]) = three();
    assert_eq!(d.resolve_view(right).unwrap().line_group, Setting { value: LineGroup::Mm0_5, set_by: SetBy::Drawing });
    d.line_group = LineGroup::Mm1;
    assert_eq!(d.resolve_view(right).unwrap().line_group, Setting { value: LineGroup::Mm1, set_by: SetBy::Drawing });
    Arc::make_mut(&mut d.sheets[0]).line_group = Some(LineGroup::Mm0_35);
    for view in [front, right] {
        assert_eq!(d.resolve_view(view).unwrap().line_group, Setting { value: LineGroup::Mm0_35, set_by: SetBy::Sheet });
    }
}

/// A view's own line group beats its sheet's, and the views projected from it follow it.
#[test]
fn a_views_own_line_group_beats_its_sheets_and_its_children_follow() {
    let (mut d, _, [front, right, top]) = three();
    Arc::make_mut(&mut d.sheets[0]).line_group = Some(LineGroup::Mm0_35);
    view_mut(&mut d, right).settings.line_group = Some(LineGroup::Mm0_7);
    assert_eq!(d.resolve_view(right).unwrap().line_group, Setting { value: LineGroup::Mm0_7, set_by: SetBy::This });
    assert_eq!(d.resolve_view(top).unwrap().line_group, Setting { value: LineGroup::Mm0_7, set_by: SetBy::Parent(right) });
    assert_eq!(d.resolve_view(front).unwrap().line_group.set_by, SetBy::Sheet);
    // Saved with the view, and read back.
    let v = file(&d);
    let saved = v["views"].as_array().unwrap().iter().find(|v| v["id"] == json!(ids::encode(right))).unwrap();
    assert_eq!(saved["settings"]["line_group"], json!("0.7"));
    assert_eq!(Drawing::from_json(&v.to_string()).unwrap().resolve_view(top).unwrap().line_group.value, LineGroup::Mm0_7);
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
