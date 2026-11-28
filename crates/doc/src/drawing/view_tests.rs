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
use crate::drawing::cutting_line::{ArrowSide, CuttingLine, SectionKind};
use crate::drawing::{Excluded, Sheet, Source};
use crate::drawing_view::{Exploded, JointPose, OccurrencePaths, Pose, StudyStep, ViewConfiguration};
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
    d.sources.push(Source { id: source, path: None });
    let front = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
    let id = add(&mut d, &mut id_source, front, ViewPlace::At([150.0, 150.0]), scaled("1:2"));
    (d, id_source, id)
}

/// A view on the drawing's first sheet, showing no more for its reference parts.
fn add(d: &mut Drawing, id_source: &mut IdSource, kind: ViewKind, place: ViewPlace, settings: ViewOverrides) -> u64 {
    let id = d.allocate_id(id_source).unwrap();
    let sheet = d.sheets[0].id;
    d.add_views([Arc::new(View { id, sheet, kind, place, settings, reference_shown_percent: 0.0, centre_marks: None, label: None })]);
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

/// A view's configuration saves as the row's id, as an exploded view's choice saves its view, and
/// follows down its chain until a view sets its own. "active" is a setting of its own: a child can
/// draw the active configuration under a parent that draws another. A view that sets none saves
/// nothing and draws the active one.
#[test]
fn a_configuration_follows_its_parent_and_saves_by_its_row_id() {
    let (mut d, _, [front, right, top]) = three();
    let resolved = |d: &Drawing, id: u64| d.resolve_view(id).unwrap().configuration;
    assert_eq!(resolved(&d, top), Setting { value: ViewConfiguration::Active, set_by: SetBy::Default });
    view_mut(&mut d, front).settings.configuration = Some(ViewConfiguration::Row(17));
    d.validate().unwrap();
    assert_eq!(resolved(&d, right), Setting { value: ViewConfiguration::Row(17), set_by: SetBy::Parent(front) });
    assert_eq!(resolved(&d, top), Setting { value: ViewConfiguration::Row(17), set_by: SetBy::Parent(front) });
    view_mut(&mut d, right).settings.configuration = Some(ViewConfiguration::Active);
    assert_eq!(resolved(&d, top), Setting { value: ViewConfiguration::Active, set_by: SetBy::Parent(right) });
    assert_eq!(resolved(&d, front), Setting { value: ViewConfiguration::Row(17), set_by: SetBy::This });
    let v = file(&d);
    let saved = |id: u64| v["views"].as_array().unwrap().iter().find(|view| view["id"] == json!(ids::encode(id))).cloned().unwrap();
    assert_eq!(saved(front)["settings"], json!({"scale": "1:2", "configuration": {"row": 17}}));
    assert_eq!(saved(right)["settings"]["configuration"], json!("active"));
    assert_eq!(saved(top)["settings"].get("configuration"), None);
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), d.to_json().unwrap());
}

/// A saved configuration reads as "active" or a row's id, whatever the id: a row the design no
/// longer has reads (the view says why where it draws, never refuses the file). Anything else is
/// refused, a row's name included: the file names rows by id.
#[test]
fn a_configuration_reads_as_active_or_a_row_id() {
    let (d, _, _) = three();
    let at = d.views.iter().position(|view| matches!(view.kind, ViewKind::Base { .. })).unwrap();
    let with = |configuration: Value| {
        refused(&d, |v| {
            v["views"][at]["settings"]["configuration"] = configuration;
        })
    };
    // What each reads as: "read", or a part of why it is refused ("" for refused, whatever the words).
    let cases = [
        (json!("active"), "read"),
        (json!(null), "read"),
        (json!({"row": 0}), "read"),
        (json!({"row": 999_999}), "read"),
        (json!({"row": u64::MAX}), "read"),
        (json!({"row": -1}), "invalid value"),
        (json!({"row": 1.5}), "invalid type"),
        (json!({"row": "Tall"}), "invalid type"),
        (json!("Tall"), "unknown variant"),
        (json!("Active"), "unknown variant"),
        (json!({"row": 3, "view": 4}), ""),
        (json!({"rows": [3]}), "unknown variant"),
        (json!(3), ""),
        (json!([]), ""),
    ];
    for (configuration, says) in cases {
        let e = with(configuration.clone());
        assert!(if says.is_empty() { e != "read" } else { e.contains(says) }, "{configuration}: {e}");
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
        v["views"][at]["settings"]["configuration"] = json!({"row": id(seed, 300)});
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

#[test]
fn automatic_centre_mark_rule_round_trips_and_keeps_missing_references() {
    let (mut d, _, id) = front();
    assert!(file(&d)["views"][0].get("centre_marks").is_none());
    let edge = ModelEdge { body: "Plate".into(), origin_feature: 1, name: "top|hole".into(), path: vec![7, 9] };
    view_mut(&mut d, id).centre_marks = Some(AutoMarks { exclude: vec![edge.clone()] });
    let saved = d.to_json().unwrap();
    let reopened = Drawing::from_json(&saved).unwrap();
    assert_eq!(reopened.view(id).unwrap().centre_marks.as_ref().unwrap().exclude, vec![edge]);
    assert_eq!(reopened.to_json().unwrap(), saved);
    let mut old = file(&d);
    old["views"][0].as_object_mut().unwrap().remove("centre_marks");
    let old = Drawing::from_json(&old.to_string()).unwrap();
    assert!(old.view(id).unwrap().centre_marks.is_none());
    // No model lookup is required to read a lost exclusion.
    assert!(reopened.validate().is_ok());
}

#[test]
fn automatic_centre_mark_rule_refuses_invalid_or_duplicate_exclusions() {
    let (mut d, _, id) = front();
    let edge = ModelEdge { body: "Plate".into(), origin_feature: 1, name: "top|hole".into(), path: vec![7] };
    view_mut(&mut d, id).centre_marks = Some(AutoMarks { exclude: vec![edge.clone()] });
    assert!(d.validate().is_ok());
    let mut other = edge.clone();
    other.path = vec![8];
    view_mut(&mut d, id).centre_marks.as_mut().unwrap().exclude.push(other);
    assert!(d.validate().is_ok(), "different occurrences are distinct");
    view_mut(&mut d, id).centre_marks.as_mut().unwrap().exclude.push(edge);
    assert!(d.validate().unwrap_err().to_string().contains("listed twice"));
    view_mut(&mut d, id).centre_marks.as_mut().unwrap().exclude =
        vec![ModelEdge { body: String::new(), origin_feature: 0, name: "edge".into(), path: vec![] }];
    assert!(d.validate().is_err());
    view_mut(&mut d, id).centre_marks.as_mut().unwrap().exclude =
        vec![ModelEdge { body: "Plate".into(), origin_feature: 1, name: "edge".into(), path: vec![1] }; MAX_MARK_EXCLUSIONS + 1];
    assert!(d.validate().unwrap_err().to_string().contains("at most"));
}

#[test]
fn complete_rim_uses_angular_union_not_sum() {
    use crate::drawing::auto_marks::full_rim;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    let tol = 1e-8;
    assert!(full_rim(&[(0.0, PI), (PI, PI)], tol));
    assert!(full_rim(&[(PI, -PI), (0.0, -PI)], tol));
    assert!(full_rim(&[(FRAC_PI_2, PI), (3.0 * FRAC_PI_2, PI)], tol));
    assert!(full_rim(&[(0.3, TAU)], tol));
    assert!(!full_rim(&[(0.0, PI), (0.0, PI)], tol), "overlapping halves do not cover a circle");
    assert!(!full_rim(&[(0.0, FRAC_PI_2)], tol));
    assert!(full_rim(&[(0.0, PI), (PI + tol / 2.0, PI - tol)], tol));
    assert!(!full_rim(&[(0.0, PI), (PI + 2.0 * tol, PI - 2.0 * tol)], tol));
    assert!(!full_rim(&[(tol * 0.75, TAU - tol * 1.5)], tol), "both ends form one gap across the seam");
    assert!(!full_rim(&[(0.0, PI), (f64::NAN, PI)], tol));
    assert!(!full_rim(&[(0.0, f64::INFINITY)], tol));
    assert!(!full_rim(&[(0.0, 2.0 * TAU)], tol));
    assert!(!full_rim(&[], tol));
    assert!(!full_rim(&[(0.0, TAU)], f64::NAN));
    assert!(!full_rim(&vec![(0.0, TAU); crate::drawing::auto_marks::MAX_RIM_ARCS + 1], tol));
}

#[test]
fn auto_mark_grouping_requires_complete_rims_and_topological_association() {
    use crate::drawing::auto_marks::{CircularEdge, group_candidates};
    use crate::drawing::dimension::ModelEdge;
    use solidcore_record::Vec2;
    use std::f64::consts::{PI, TAU};

    let edge = |name: &str, radius, depth, faces: &[u32], path: &[u64], spans: Vec<(f64, f64)>| CircularEdge {
        edge: ModelEdge { body: "Part".into(), origin_feature: 1, name: name.into(), path: path.to_vec() },
        index: name.as_bytes().first().copied().unwrap_or_default() as usize,
        faces: faces.to_vec(),
        center: Vec2::new(10.0, 20.0),
        radius,
        depth,
        spans,
        exact: true,
    };
    let candidates = vec![
        edge("a", 6.0, 0.0, &[1, 2], &[], vec![(0.0, PI)]),
        edge("b", 6.0, 0.0, &[1, 3], &[], vec![(PI, PI)]),
        edge("c", 6.0, 40.0, &[2, 9], &[], vec![(0.0, TAU)]),
        edge("d", 10.0, 0.0, &[9, 4], &[], vec![(0.0, TAU)]),
        edge("e", 6.0, 0.0, &[90], &[], vec![(0.0, TAU)]),
        edge("f", 6.0, 0.0, &[1, 2], &[5], vec![(0.0, TAU)]),
        edge("g", 3.0, 0.0, &[80], &[], vec![(0.0, PI / 2.0)]),
        edge("h", 4.0, 0.0, &[70], &[], vec![(0.0, PI)]),
        edge("i", 4.0, 0.0, &[70], &[], vec![(0.0, PI)]),
    ];
    let grouped = group_candidates(&candidates).unwrap();
    assert_eq!(grouped.len(), 3);
    let bore = grouped.iter().find(|mark| mark.radius == 10.0).unwrap();
    assert_eq!(bore.sources.iter().map(|edge| edge.name.as_str()).collect::<Vec<_>>(), ["a", "b", "c", "d"]);
    assert!(grouped.iter().any(|mark| mark.sources.len() == 1 && mark.sources[0].name == "e"));
    assert!(grouped.iter().any(|mark| mark.sources.len() == 1 && mark.sources[0].path == [5]));
    let mut reversed = candidates.clone();
    reversed.reverse();
    assert_eq!(group_candidates(&reversed).unwrap(), grouped);
    let mut invalid = candidates.clone();
    invalid[0].spans[0].0 = f64::NAN;
    assert!(group_candidates(&invalid).is_err());
}

/// View `id`'s letter, if it has one.
fn letter(d: &Drawing, id: u64) -> Option<String> {
    d.view(id)?.label.as_ref().map(|label| label.letter.clone())
}

/// A view placed apart from its projection place gets the first letter no view holds, I, O and Q
/// skipped; one lined up with its parent or a base view gets none. A freed letter comes back while
/// letters are reused, never once they aren't, and the letters given are counted either way.
#[test]
fn a_view_placed_apart_gets_the_first_free_letter() {
    let (mut d, mut id_source, [front, right, top]) = three();
    for id in [front, right, top] {
        d.give_letter(id).unwrap();
    }
    assert_eq!([front, right, top].map(|id| letter(&d, id)), [None, None, Some("A".into())]);
    let label = d.view(top).unwrap().label.clone().unwrap();
    assert_eq!((label.words.as_str(), label.scale_words.as_str(), label.shown, label.offset), ("VIEW", "SCALE", true, None));
    let mut apart = |d: &mut Drawing| {
        let id = add(d, &mut id_source, projected(front, ProjectedSide::Below), ViewPlace::At([150.0, 40.0]), ViewOverrides::default());
        d.give_letter(id).unwrap();
        id
    };
    let more: Vec<u64> = (0..8).map(|_| apart(&mut d)).collect();
    let given: Vec<String> = more.iter().filter_map(|id| letter(&d, *id)).collect();
    assert_eq!(given, ["B", "C", "D", "E", "F", "G", "H", "J"], "I is skipped");
    assert_eq!(d.view_letters, ViewLetters { reuse: true, given: 9 });
    // A letter already there stays; a view freed of its letter's holder gives it back.
    d.give_letter(top).unwrap();
    assert_eq!(letter(&d, top).as_deref(), Some("A"));
    d.views.retain(|v| v.id != top);
    let again = apart(&mut d);
    assert_eq!(letter(&d, again).as_deref(), Some("A"), "a freed letter comes back");
    d.views.retain(|v| v.id != more[0]);
    d.view_letters.reuse = false;
    let past = apart(&mut d);
    assert_eq!(letter(&d, past).as_deref(), Some("K"), "never again once letters aren't reused: B stays free");
    assert_eq!(d.view_letters.given, 10);
    d.validate().unwrap();
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened.view_letters, d.view_letters, "saved");
}

/// A view's label reads `VIEW` and its letter, its name, then its scale when it isn't its sheet's,
/// each part as typed and only when shown; a view in its projection place, or one whose label is
/// hidden, draws none and keeps its letter.
#[test]
fn a_views_label_reads_its_shown_parts() {
    let (mut d, _, [front, right, top]) = three();
    d.give_letter(top).unwrap();
    assert_eq!(d.label_text(top).as_deref(), Some("VIEW A"));
    assert_eq!((d.label_text(front), d.label_text(right)), (None, None), "no label");
    view_mut(&mut d, top).settings.scale = Some("2:1".parse().unwrap());
    assert_eq!(d.label_text(top).as_deref(), Some("VIEW A\nSCALE 2:1"), "the sheet's scale is its base view's, 1:2");
    let label = view_mut(&mut d, top).label.as_mut().unwrap();
    label.words = String::new();
    label.name = "BRACKET END".into();
    label.scale_words = "ECHELLE".into();
    assert_eq!(d.label_text(top).as_deref(), Some("A\nBRACKET END\nECHELLE 2:1"));
    let label = view_mut(&mut d, top).label.as_mut().unwrap();
    (label.show_letter, label.show_scale) = (false, false);
    assert_eq!(d.label_text(top).as_deref(), Some("BRACKET END"));
    view_mut(&mut d, top).label.as_mut().unwrap().name.clear();
    assert_eq!(d.label_text(top), None, "nothing shown");
    view_mut(&mut d, top).label.as_mut().unwrap().show_letter = true;
    view_mut(&mut d, top).label.as_mut().unwrap().shown = false;
    assert_eq!((d.label_text(top), letter(&d, top).as_deref()), (None, Some("A")), "hidden, its letter kept");
    view_mut(&mut d, top).label.as_mut().unwrap().shown = true;
    view_mut(&mut d, top).place = ViewPlace::Aligned(320.0);
    assert_eq!((d.label_text(top), letter(&d, top).as_deref()), (None, Some("A")), "back in its projection place");
}

/// A label's letter is one to three capitals, its words one line each, and its offset a point on
/// paper; a letter two views hold opens (a hand edit), and giving one of them a letter gives it
/// another.
#[test]
fn a_label_keeps_the_records_rules_and_a_doubled_letter_opens() {
    let (mut d, mut id_source, [front, _, top]) = three();
    d.give_letter(top).unwrap();
    let at = |v: &mut Value| v["views"].as_array_mut().unwrap().iter_mut().find(|v| v["label"].is_object()).unwrap()["label"].take();
    let with = |d: &Drawing, key: &str, value: Value| {
        refused(d, |v| {
            let mut label = at(v);
            label[key] = value;
            let views = v["views"].as_array_mut().unwrap();
            views.iter_mut().find(|v| v["label"].is_null() && v["place"]["at"].is_array() && v["kind"]["projected"].is_object()).unwrap()["label"] =
                label;
        })
    };
    assert_eq!(with(&d, "letter", json!("B")), "read");
    for (key, value) in [("letter", json!("a")), ("letter", json!("ABCD")), ("letter", json!("")), ("letter", json!("A1"))] {
        assert!(with(&d, key, value.clone()).contains("its letter is one to three capital letters A to Z"), "{key}: {value}");
    }
    assert!(with(&d, "name", json!("two\nlines")).contains("a label is one line"));
    assert!(with(&d, "words", json!("x".repeat(5000))).contains("a label is one line"));
    for offset in [json!([1e9, 0.0]), json!([0.0, -6000.0])] {
        assert!(with(&d, "offset", offset.clone()).contains("its label's offset is at most 5000 mm"), "{offset}");
    }
    let twin = add(&mut d, &mut id_source, projected(front, ProjectedSide::Below), ViewPlace::At([150.0, 40.0]), ViewOverrides::default());
    view_mut(&mut d, twin).label = d.view(top).unwrap().label.clone();
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(letter(&reopened, twin).as_deref(), Some("A"), "a doubled letter opens");
    d.give_letter(twin).unwrap();
    assert_eq!((letter(&d, top).as_deref(), letter(&d, twin).as_deref()), (Some("A"), Some("B")), "the copy takes the next");
}

/// [`three`], with a view lined up to the right of the right view (centre (450, 150)) and one lined up
/// below the front view (centre (150, 60)): the drawing, its id source, and the front, right, top,
/// back and below views' ids.
fn lined() -> (Drawing, IdSource, [u64; 5]) {
    let (mut d, mut id_source, [front, right, top]) = three();
    let back = add(&mut d, &mut id_source, projected(right, ProjectedSide::Right), ViewPlace::Aligned(450.0), ViewOverrides::default());
    let below = add(&mut d, &mut id_source, projected(front, ProjectedSide::Below), ViewPlace::Aligned(60.0), ViewOverrides::default());
    d.validate().unwrap();
    (d, id_source, [front, right, top, back, below])
}

/// `first`, then `rest` in id order: how a move lists the views it moved.
fn then_by_id<T>(first: (u64, T), mut rest: Vec<(u64, T)>) -> Vec<(u64, T)> {
    rest.sort_unstable_by_key(|(id, _)| *id);
    std::iter::once(first).chain(rest).collect()
}

/// `first`, then `rest` in id order, as ids alone.
fn ids_then_by_id(first: u64, rest: &[u64]) -> Vec<u64> {
    then_by_id((first, ()), rest.iter().map(|id| (*id, ())).collect()).into_iter().map(|(id, _)| id).collect()
}

/// The views lined up with a view are found down its line, through others and in id order; a view
/// placed apart ends the line, and so does what is lined up with it. All are still projected from it.
#[test]
fn the_views_lined_up_with_a_view_are_found_down_its_line() {
    let (mut d, mut id_source, [front, right, top, back, below]) = lined();
    let beside_top = add(&mut d, &mut id_source, projected(top, ProjectedSide::Right), ViewPlace::Aligned(450.0), ViewOverrides::default());
    d.validate().unwrap();
    let mut in_order = vec![right, back, below];
    in_order.sort_unstable();
    assert_eq!(d.lined_up_with(front), in_order);
    assert_eq!(d.lined_up_with(right), [back]);
    assert_eq!(d.lined_up_with(top), [beside_top]);
    assert_eq!(d.lined_up_with(back), Vec::<u64>::new());
    assert_eq!(d.lined_up_with(99), Vec::<u64>::new());
    let all = d.with_projected(&BTreeSet::from([front]));
    assert_eq!(all, BTreeSet::from([front, right, top, back, below, beside_top]), "placed apart or not");
}

/// A view moved on its sheet goes where it is dragged, and the views lined up with it follow by the
/// record's rule with their records as they were: the answer is it, then those whose centres changed.
/// A view placed apart stays.
#[test]
fn a_moved_view_takes_its_line_up_and_says_which_views_moved() {
    let (mut d, _, [front, right, top, back, below]) = lined();
    let records = |d: &Drawing| [right, top, back, below].map(|id| d.view(id).unwrap().place);
    let before = records(&d);
    let moved = d.move_view(front, Vec2::new(170.0, 155.0), false).unwrap();
    assert_eq!(moved, ids_then_by_id(front, &[right, back, below]));
    assert_eq!(d.view(front).unwrap().place, ViewPlace::At([170.0, 155.0]));
    let centres = |d: &Drawing| [front, right, top, back, below].map(|id| d.view_centre(id).unwrap());
    let at = |x, y| Vec2::new(x, y);
    assert_eq!(centres(&d), [at(170.0, 155.0), at(320.0, 155.0), at(320.0, 250.0), at(450.0, 155.0), at(170.0, 60.0)]);
    assert_eq!(records(&d), before, "they follow by the rule, not by a write");
    // Across only: the views to its right keep their centres, so only the one below it moved.
    assert_eq!(d.move_view(front, Vec2::new(180.0, 155.0), false).unwrap(), [front, below]);
    assert_eq!(d.view_centre(below).unwrap(), at(180.0, 60.0));
}

/// A view lined up with its parent takes only its free axis, and with `break_alignment` is placed apart
/// where it was dragged, its own line-up following; a view placed at a point takes both axes either
/// way. A move to where it is changes nothing, and an unknown view is refused and changes nothing.
#[test]
fn a_lined_up_view_keeps_to_its_line_unless_broken() {
    let (mut d, _, [front, right, top, back, below]) = lined();
    let at = |x, y| Vec2::new(x, y);
    assert_eq!(d.move_view(right, at(340.0, 999.0), false).unwrap(), [right], "the view beside it keeps its centre");
    assert_eq!((d.view(right).unwrap().place, d.view_centre(right).unwrap()), (ViewPlace::Aligned(340.0), at(340.0, 150.0)));
    assert_eq!(letter(&d, right), None, "moved along its line, it needs no letter");
    assert_eq!(d.move_view(below, at(999.0, 40.0), false).unwrap(), [below]);
    assert_eq!((d.view(below).unwrap().place, d.view_centre(below).unwrap()), (ViewPlace::Aligned(40.0), at(150.0, 40.0)));
    assert_eq!(d.move_view(right, at(340.0, 90.0), true).unwrap(), [right, back]);
    assert_eq!(d.view(right).unwrap().place, ViewPlace::At([340.0, 90.0]));
    assert!(letter(&d, right).is_some(), "placed apart, it is given its letter by the move itself");
    assert_eq!((d.view_centre(back).unwrap(), d.view_centre(front).unwrap()), (at(450.0, 90.0), at(150.0, 150.0)), "its parent stays");
    assert_eq!(d.move_view(top, at(330.0, 260.0), true).unwrap(), [top]);
    assert_eq!(d.view(top).unwrap().place, ViewPlace::At([330.0, 260.0]));
    let before = d.clone();
    assert_eq!(d.move_view(front, at(150.0, 150.0), false).unwrap(), [front]);
    assert_eq!(d, before, "where it is: nothing changes");
    assert_eq!(d.move_view(99, at(1.0, 1.0), false).unwrap_err().to_string(), format!("unknown view {}", ids::encode(99)));
    assert_eq!(d, before);
}

/// A view placed apart at a point takes the views lined up with it, each moved along its free axis as
/// it moved, so they keep their places beside it; a view placed apart from it isn't one of them.
#[test]
fn a_view_placed_apart_takes_its_line_up_beside_it() {
    let (d, _, [front, right, _, back, below]) = lined();
    let aligned = |id, along| (id, ViewPlace::Aligned(along));
    let moved = vec![aligned(right, 270.0), aligned(back, 400.0), aligned(below, 30.0)];
    assert_eq!(d.places_beside(front, Vec2::new(100.0, 120.0)).unwrap(), then_by_id((front, ViewPlace::At([100.0, 120.0])), moved));
    assert_eq!(d.places_beside(right, Vec2::new(200.0, 100.0)).unwrap(), [(right, ViewPlace::At([200.0, 100.0])), aligned(back, 330.0)]);
    assert!(d.places_beside(99, Vec2::new(1.0, 1.0)).is_err());
}

/// A full cutting line, horizontal at height 5, its arrows to its right.
fn cutting() -> CuttingLine {
    CuttingLine::new(SectionKind::Full, vec![Vec2::new(-30.0, 5.0), Vec2::new(30.0, 5.0)], ArrowSide::Right).unwrap()
}

/// A section view of `parent`, cut along [`cutting`].
fn section(parent: u64) -> ViewKind {
    ViewKind::Section { parent, line: cutting() }
}

/// A section view of a projected view resolves through its chain: its basis is its line's turn of its
/// parent's, its cut is that line in its parent's plane, its settings follow its parents; a view
/// projected from it isn't cut. It saves its line in its kind and reads back the same bytes; it is
/// taken with its parent, needs and gets its letter.
#[test]
fn a_section_view_resolves_saves_and_goes_with_its_parent() {
    let (mut d, mut id_source, [front, right, top]) = three();
    let cut = add(&mut d, &mut id_source, section(right), ViewPlace::At([320.0, 60.0]), ViewOverrides::default());
    let beyond = add(&mut d, &mut id_source, projected(cut, ProjectedSide::Right), ViewPlace::At([400.0, 60.0]), ViewOverrides::default());
    d.validate().unwrap();
    let line = cutting();
    let right_basis = d.resolve_view(right).unwrap().basis;
    let resolved = d.resolve_view(cut).unwrap();
    assert_eq!(resolved.basis, line.basis(&right_basis).unwrap());
    assert_eq!(resolved.cut, Some(Cut { line: line.clone(), parent: right_basis }));
    assert_eq!(resolved.scale, Setting { value: "1:2".parse().unwrap(), set_by: SetBy::Parent(front) });
    assert_eq!(resolved.visual_style, Setting { value: VisualStyle::VisibleHidden, set_by: SetBy::Parent(right) });
    let after = d.resolve_view(beyond).unwrap();
    assert_eq!((after.cut, after.basis), (None, projected_basis(&resolved.basis, ProjectedSide::Right, Projection::Third).unwrap()));
    assert_eq!(d.resolve_view(top).unwrap().cut, None);
    // Saved in its kind, the same bytes again.
    let v = file(&d);
    let saved = v["views"].as_array().unwrap().iter().find(|view| view["id"] == json!(ids::encode(cut))).unwrap();
    assert_eq!(saved["kind"]["section"]["parent"], json!(ids::encode(right)));
    assert_eq!(saved["kind"]["section"]["line"], serde_json::to_value(&line).unwrap());
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), d.to_json().unwrap());
    // Taken with its parent and its parent's parents; it takes what is projected from it.
    let set = |ids: &[u64]| ids.iter().copied().collect::<BTreeSet<u64>>();
    assert_eq!(d.with_projected(&set(&[front])), set(&[front, right, top, cut, beyond]));
    assert_eq!(d.with_projected(&set(&[cut])), set(&[cut, beyond]));
    assert_eq!(d.view(cut).unwrap().kind.parent(), Some(right));
    assert_eq!(d.view(front).unwrap().kind.parent(), None);
    // It needs its letter wherever it is, and gets the next one.
    d.give_letter(top).unwrap();
    d.give_letter(cut).unwrap();
    assert!(d.view(cut).unwrap().needs_letter());
    assert_eq!((letter(&d, top).as_deref(), letter(&d, cut).as_deref()), (Some("A"), Some("B")));
    d.validate().unwrap();
}

/// A section view keeps its rules on reading: placed at a point, a line that keeps the cutting line's
/// rules, a parent the drawing has, no unknown field.
#[test]
fn a_section_views_rules_are_kept_on_reading() {
    let (mut d, mut id_source, [front, ..]) = three();
    let cut = add(&mut d, &mut id_source, section(front), ViewPlace::At([150.0, 60.0]), ViewOverrides::default());
    d.validate().unwrap();
    let at = file(&d)["views"].as_array().unwrap().iter().position(|view| view["id"] == json!(ids::encode(cut))).unwrap();
    let named = |why: &str| format!("damaged drawing: view {}: {why}", ids::encode(cut));
    assert_eq!(
        refused(&d, |v| v["views"][at]["place"] = json!({"aligned": 40.0})),
        named("a section view is placed at a point: it has no side to line up on")
    );
    let why = refused(&d, |v| v["views"][at]["kind"]["section"]["line"]["points"] = json!([[0.0, 0.0], [0.0, 0.0]]));
    assert!(why.contains("a cutting line's points are at least 0.000001 mm apart"), "{why}");
    let why = refused(&d, |v| v["views"][at]["kind"]["section"]["line"]["kind"] = json!("half"));
    assert!(why.contains("a half section's cutting line has 3 points"), "{why}");
    let gone = ids::encode(99);
    assert_eq!(
        refused(&d, |v| v["views"][at]["kind"]["section"]["parent"] = json!(gone)),
        named(&format!("its parent {gone} isn't one of the drawing's views"))
    );
    assert_ne!(refused(&d, |v| v["views"][at]["kind"]["section"]["side"] = json!("right")), "read");
    assert_eq!(refused(&d, |_| {}), "read");
}

fn pictorial(parent: u64, diagonal: Diagonal) -> ViewKind {
    ViewKind::Pictorial { parent, diagonal }
}

/// The three views and a pictorial view above right of the front view, placed apart.
fn four() -> (Drawing, IdSource, [u64; 4]) {
    let (mut d, mut id_source, [front, right, top]) = three();
    let iso = add(&mut d, &mut id_source, pictorial(front, Diagonal::AboveRight), ViewPlace::At([60.0, 250.0]), ViewOverrides::default());
    d.validate().unwrap();
    (d, id_source, [front, right, top, iso])
}

/// A pictorial view saves its parent and its diagonal and its point, and reads back as saved, the
/// same bytes. It resolves to the named isometric view above right of the front view in third
/// angle (the corner between the top and right views), and in first angle the same saved view
/// looks from the opposite corner; its scale follows its parent. It is found down its parent's
/// chains, and its centre is its own point, which stays where it is when the parent moves.
#[test]
fn a_pictorial_view_saves_its_parent_and_diagonal_and_resolves_isometric() {
    let (mut d, _, [front, _, _, iso]) = four();
    let [front_id, iso_id] = [front, iso].map(ids::encode);
    let saved = file(&d)["views"].as_array().unwrap().iter().find(|v| v["id"] == json!(iso_id)).cloned().unwrap();
    assert_eq!(saved["kind"], json!({"pictorial": {"parent": front_id, "diagonal": "above_right"}}));
    assert_eq!(saved["place"], json!({"at": [60.0, 250.0]}));
    assert_eq!(saved.get("settings"), None);
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), d.to_json().unwrap());

    let resolved = d.resolve_view(iso).unwrap();
    assert_eq!(resolved.basis, NamedView::IsoFrontRight.basis());
    assert_eq!((resolved.scale.value.to_string(), resolved.scale.set_by), ("1:2".to_string(), SetBy::Parent(front)));
    d.projection = Projection::First;
    let first = d.resolve_view(iso).unwrap().basis;
    assert_eq!(first, pictorial_basis(&NamedView::Front.basis(), (1.0, 1.0), Projection::First).unwrap());
    assert_ne!(first, NamedView::IsoFrontRight.basis());
    d.projection = Projection::Third;

    let set = |ids: &[u64]| ids.iter().copied().collect::<BTreeSet<u64>>();
    assert!(d.with_projected(&set(&[front])).contains(&iso));
    assert_eq!(d.with_projected(&set(&[iso])), set(&[iso]));
    assert_eq!(d.view_centre(iso).unwrap(), Vec2::new(60.0, 250.0));
    view_mut(&mut d, front).place = ViewPlace::At([100.0, 100.0]);
    assert_eq!(d.view_centre(iso).unwrap(), Vec2::new(60.0, 250.0));
}

/// The record's rules for a pictorial view: it is placed at a point, its parent is one of the
/// drawing's views, and no view, projected, section or pictorial, is made from a pictorial view yet.
#[test]
fn a_pictorial_views_rules_are_kept() {
    let (d, mut id_source, [front, _, _, iso]) = four();
    let why = |d: &Drawing| d.validate().unwrap_err().to_string();
    let mut lined_up = d.clone();
    view_mut(&mut lined_up, iso).place = ViewPlace::Aligned(60.0);
    assert_eq!(why(&lined_up), format!("view {}: a pictorial view is placed at a point: it isn't lined up with its parent", ids::encode(iso)));
    assert_eq!(lined_up.view_centre(iso).unwrap_err().to_string(), format!("pictorial view {} isn't placed at a point", ids::encode(iso)));
    let mut orphan = d.clone();
    view_mut(&mut orphan, iso).kind = pictorial(99, Diagonal::BelowLeft);
    assert_eq!(why(&orphan), format!("view {}: its parent {} isn't one of the drawing's views", ids::encode(iso), ids::encode(99)));
    for kind in [projected(iso, ProjectedSide::Right), section(iso), pictorial(iso, Diagonal::AboveLeft)] {
        let mut from_iso = d.clone();
        let child = add(&mut from_iso, &mut id_source, kind, ViewPlace::At([10.0, 10.0]), ViewOverrides::default());
        assert_eq!(why(&from_iso), format!("view {}: not supported yet: a view projected from a pictorial view", ids::encode(child)));
        assert_eq!(from_iso.can_project_from(iso).unwrap_err().to_string(), "not supported yet: a view projected from a pictorial view");
    }
    assert!(d.can_project_from(front).is_ok());
    assert_eq!(d.can_project_from(99).unwrap_err().to_string(), format!("the drawing has no view {}", ids::encode(99)));
}

/// A pictorial view counts in its chain: one past the deepest is refused as a projected view is,
/// and its parent then has no room for one.
#[test]
fn a_pictorial_view_is_at_most_its_chains_depth() {
    let (mut d, mut id_source, front) = front();
    let mut last = front;
    for _ in 2..MAX_VIEW_DEPTH {
        last = add(&mut d, &mut id_source, projected(last, ProjectedSide::Right), ViewPlace::At([10.0, 10.0]), ViewOverrides::default());
    }
    let mut room = d.clone();
    add(&mut room, &mut id_source, pictorial(last, Diagonal::AboveRight), ViewPlace::At([10.0, 10.0]), ViewOverrides::default());
    room.validate().unwrap();
    let deepest = add(&mut d, &mut id_source, projected(last, ProjectedSide::Right), ViewPlace::At([10.0, 10.0]), ViewOverrides::default());
    assert_eq!(d.can_project_from(deepest).unwrap_err().to_string(), format!("a chain of views is at most {MAX_VIEW_DEPTH} views"));
    let past = add(&mut d, &mut id_source, pictorial(deepest, Diagonal::AboveRight), ViewPlace::At([10.0, 10.0]), ViewOverrides::default());
    assert_eq!(d.validate().unwrap_err().to_string(), format!("view {}: a chain of views is at most {MAX_VIEW_DEPTH} views", ids::encode(past)));
}

/// An offset from the parent's centre gives the nearest of eight directions: a side within 22.5°
/// of its axis (a lined-up view wins the tie on the 22.5° line), else the diagonal of its signs;
/// nothing on the centre or for an offset not finite.
#[test]
fn an_offset_picks_the_nearest_of_eight_directions() {
    use Diagonal::{AboveLeft, AboveRight, BelowLeft, BelowRight};
    use ProjectedSide::{Above, Below, Left, Right};
    let side = |s| Some(Beside::Side(s));
    let diagonal = |d| Some(Beside::Diagonal(d));
    let tie = std::f64::consts::SQRT_2 - 1.0;
    for ((dx, dy), want) in [
        ((80.0, 0.0), side(Right)),
        ((-80.0, 0.0), side(Left)),
        ((0.0, 80.0), side(Above)),
        ((0.0, -80.0), side(Below)),
        ((80.0, 80.0), diagonal(AboveRight)),
        ((-80.0, 80.0), diagonal(AboveLeft)),
        ((80.0, -80.0), diagonal(BelowRight)),
        ((-80.0, -80.0), diagonal(BelowLeft)),
        ((80.0, 33.0), side(Right)),
        ((80.0, 34.0), diagonal(AboveRight)),
        ((33.0, 80.0), side(Above)),
        ((34.0, 80.0), diagonal(AboveRight)),
        ((-33.0, -80.0), side(Below)),
        ((1.0, tie), side(Right)),
        ((tie, -1.0), side(Below)),
        ((1e-300, 0.0), side(Right)),
        ((0.0, 0.0), None),
        ((-0.0, 0.0), None),
        ((f64::NAN, 1.0), None),
        ((1.0, f64::INFINITY), None),
    ] {
        assert_eq!(Beside::of_offset(Vec2::new(dx, dy)), want, "({dx}, {dy})");
    }
}

/// The eight names read and write as plain words, sides and diagonals alike, and each kind of
/// view is named for what it is; other words are refused.
#[test]
fn beside_reads_and_writes_the_eight_names() {
    let names: Vec<Value> = Diagonal::ALL.iter().map(|d| serde_json::to_value(Beside::Diagonal(*d)).unwrap()).collect();
    assert_eq!(names, [json!("above_right"), json!("above_left"), json!("below_right"), json!("below_left")]);
    for side in ProjectedSide::ALL {
        let written = serde_json::to_value(Beside::Side(side)).unwrap();
        assert_eq!(serde_json::from_value::<Beside>(written).unwrap(), Beside::Side(side));
    }
    assert_eq!(serde_json::from_value::<Beside>(json!("below_left")).unwrap(), Beside::Diagonal(Diagonal::BelowLeft));
    for bad in [json!("top"), json!("aboveright"), json!("above-right"), json!(1)] {
        assert!(serde_json::from_value::<Beside>(bad.clone()).is_err(), "{bad}");
    }
    let (d, _, [front, right, _, iso]) = four();
    let named = |id: u64| d.view(id).map(|v| (v.kind.name(), v.kind.beside())).unwrap();
    assert_eq!(named(front), ("base", None));
    assert_eq!(named(right), ("projected", Some(Beside::Side(ProjectedSide::Right))));
    assert_eq!(named(iso), ("pictorial", Some(Beside::Diagonal(Diagonal::AboveRight))));
}

fn auxiliary(parent: u64, toward: [f64; 2]) -> ViewKind {
    ViewKind::Auxiliary { parent, toward }
}

/// The three views and an auxiliary view of the front view toward (0.6, 0.8), placed apart.
fn with_auxiliary() -> (Drawing, IdSource, [u64; 4]) {
    let (mut d, mut id_source, [front, right, top]) = three();
    let aux = add(&mut d, &mut id_source, auxiliary(front, [0.6, 0.8]), ViewPlace::At([260.0, 230.0]), ViewOverrides::default());
    d.validate().unwrap();
    (d, id_source, [front, right, top, aux])
}

/// An auxiliary view saves its parent, its side and its point, and reads back as saved, the same
/// bytes. It resolves to its parent folded toward its side: from that side in third angle, from the
/// side away in first, and toward a side it is that side's projected view. Its scale follows its
/// parent, it is found down its parent's chains, its centre is its own point, and it names itself
/// with no place of the eight. An auxiliary view of an auxiliary view resolves down its chain.
#[test]
fn an_auxiliary_view_saves_its_side_and_resolves_folded() {
    let (mut d, mut id_source, [front, _, _, aux]) = with_auxiliary();
    let [front_id, aux_id] = [front, aux].map(ids::encode);
    let saved = file(&d)["views"].as_array().unwrap().iter().find(|v| v["id"] == json!(aux_id)).cloned().unwrap();
    assert_eq!(saved["kind"], json!({"auxiliary": {"parent": front_id, "toward": [0.6, 0.8]}}));
    assert_eq!((saved["place"].clone(), saved.get("settings")), (json!({"at": [260.0, 230.0]}), None));
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), d.to_json().unwrap());

    let front_basis = NamedView::Front.basis();
    let resolved = d.resolve_view(aux).unwrap();
    assert_eq!(resolved.basis, auxiliary_basis(&front_basis, Vec2::new(0.6, 0.8), Projection::Third).unwrap());
    assert_eq!((resolved.scale.value.to_string(), resolved.scale.set_by), ("1:2".to_string(), SetBy::Parent(front)));
    d.projection = Projection::First;
    let first = d.resolve_view(aux).unwrap().basis;
    assert_eq!(first, auxiliary_basis(&front_basis, Vec2::new(0.6, 0.8), Projection::First).unwrap());
    assert_ne!(first, resolved.basis);
    d.projection = Projection::Third;
    view_mut(&mut d, aux).kind = auxiliary(front, [1.0, 0.0]);
    assert_eq!(d.resolve_view(aux).unwrap().basis, NamedView::Right.basis(), "toward the right, the right view");
    view_mut(&mut d, aux).kind = auxiliary(front, [0.6, 0.8]);

    let set = |ids: &[u64]| ids.iter().copied().collect::<BTreeSet<u64>>();
    assert!(d.with_projected(&set(&[front])).contains(&aux));
    assert_eq!(d.view_centre(aux).unwrap(), Vec2::new(260.0, 230.0));
    view_mut(&mut d, front).place = ViewPlace::At([100.0, 100.0]);
    assert_eq!(d.view_centre(aux).unwrap(), Vec2::new(260.0, 230.0), "placed apart, it stays");
    assert_eq!(d.view(aux).map(|v| (v.kind.name(), v.kind.beside(), v.kind.parent())), Some(("auxiliary", None, Some(front))));

    let second = add(&mut d, &mut id_source, auxiliary(aux, [0.0, -1.0]), ViewPlace::At([300.0, 100.0]), ViewOverrides::default());
    d.validate().unwrap();
    let folded_twice = auxiliary_basis(&resolved.basis, Vec2::new(0.0, -1.0), Projection::Third).unwrap();
    assert_eq!(d.resolve_view(second).unwrap().basis, folded_twice);
}

/// The record's rules for an auxiliary view: it is placed at a point; its side is a direction of
/// about unit length (each number finite and at most 1 either way, one at least 0.5), in the file as
/// in an edit; its parent is one of the drawing's views and not a pictorial view; and it counts in
/// its chain, which is at most [`MAX_VIEW_DEPTH`] views.
#[test]
fn an_auxiliary_views_rules_are_kept() {
    let (d, mut id_source, [front, _, _, aux]) = with_auxiliary();
    let why = |d: &Drawing| d.validate().unwrap_err().to_string();
    let mut lined_up = d.clone();
    view_mut(&mut lined_up, aux).place = ViewPlace::Aligned(60.0);
    assert_eq!(why(&lined_up), format!("view {}: an auxiliary view is placed at a point: it isn't lined up with its parent", ids::encode(aux)));
    assert_eq!(lined_up.view_centre(aux).unwrap_err().to_string(), format!("auxiliary view {} isn't placed at a point", ids::encode(aux)));
    let rule = "an auxiliary view's toward is a direction of about unit length: each number at most 1 either way, one at least 0.5";
    for toward in [[0.0, 0.0], [0.4, 0.4], [1.5, 0.0], [0.6, -1.000_001], [f64::NAN, 1.0], [1.0, f64::INFINITY], [-0.499, 0.0]] {
        let mut bad = d.clone();
        view_mut(&mut bad, aux).kind = auxiliary(front, toward);
        assert_eq!(why(&bad), format!("view {}: {rule}", ids::encode(aux)), "{toward:?}");
    }
    for toward in [[0.5, 0.0], [-1.0, 1.0], [0.0, -0.5], [-0.0, 1.0]] {
        let mut good = d.clone();
        view_mut(&mut good, aux).kind = auxiliary(front, toward);
        good.validate().unwrap();
    }
    let read = refused(&d, |v| {
        let views = v["views"].as_array_mut().unwrap();
        let saved = views.iter_mut().find(|view| view["id"] == json!(ids::encode(aux))).unwrap();
        saved["kind"]["auxiliary"]["toward"] = json!([2.0, 0.0]);
    });
    assert!(read.contains(rule), "{read}");
    let mut orphan = d.clone();
    view_mut(&mut orphan, aux).kind = auxiliary(99, [1.0, 0.0]);
    assert_eq!(why(&orphan), format!("view {}: its parent {} isn't one of the drawing's views", ids::encode(aux), ids::encode(99)));
    let mut from_iso = d.clone();
    let iso = add(&mut from_iso, &mut id_source, pictorial(front, Diagonal::AboveRight), ViewPlace::At([60.0, 250.0]), ViewOverrides::default());
    let child = add(&mut from_iso, &mut id_source, auxiliary(iso, [1.0, 0.0]), ViewPlace::At([10.0, 10.0]), ViewOverrides::default());
    assert_eq!(why(&from_iso), format!("view {}: not supported yet: a view projected from a pictorial view", ids::encode(child)));

    let (mut deep, mut id_source, front) = self::front();
    let mut last = front;
    for _ in 1..MAX_VIEW_DEPTH {
        last = add(&mut deep, &mut id_source, auxiliary(last, [0.0, 1.0]), ViewPlace::At([10.0, 10.0]), ViewOverrides::default());
    }
    deep.validate().unwrap();
    assert_eq!(deep.can_project_from(last).unwrap_err().to_string(), format!("a chain of views is at most {MAX_VIEW_DEPTH} views"));
    let past = add(&mut deep, &mut id_source, auxiliary(last, [0.0, 1.0]), ViewPlace::At([10.0, 10.0]), ViewOverrides::default());
    assert_eq!(deep.validate().unwrap_err().to_string(), format!("view {}: a chain of views is at most {MAX_VIEW_DEPTH} views", ids::encode(past)));
}
/// A drawing with a flat pattern view of body `Plate` at 1:1, placed at (300, 150) and turned
/// `turn`, beside the front view, and the flat view's id.
fn flat(turn: QuarterTurn) -> (Drawing, IdSource, u64) {
    let (mut d, mut id_source, _) = front();
    let source = d.sources[0].id;
    let kind = ViewKind::Flat { source, body: "Plate".into(), turn };
    let id = add(&mut d, &mut id_source, kind, ViewPlace::At([300.0, 150.0]), scaled("1:1"));
    d.validate().unwrap();
    (d, id_source, id)
}

/// A flat pattern view saves its source, its body and its quarter turn, in degrees, and reads back as
/// saved. It resolves to its turn's basis with its body and turn, a base view to none, and its centre
/// is its place.
#[test]
fn a_flat_view_saves_its_body_and_turn_and_resolves_to_its_basis() {
    let (d, _, id) = flat(QuarterTurn::Quarter);
    let v = file(&d);
    let saved = v["views"].as_array().unwrap().iter().find(|view| view["id"] == json!(ids::encode(id))).unwrap();
    let source = ids::encode(d.sources[0].id);
    assert_eq!(saved["kind"], json!({"flat": {"source": source, "body": "Plate", "turn": 90}}));
    assert_eq!(Drawing::from_json(&d.to_json().unwrap()).unwrap(), d);
    let resolved = d.resolve_view(id).unwrap();
    assert_eq!(resolved.basis, QuarterTurn::Quarter.basis());
    assert_eq!(resolved.flat, Some(FlatOf { body: "Plate".into(), turn: QuarterTurn::Quarter }));
    let base = d.views.iter().find(|v| matches!(v.kind, ViewKind::Base { .. })).unwrap().id;
    assert_eq!(d.resolve_view(base).unwrap().flat, None);
    assert_eq!(d.view_centre(id).unwrap(), Vec2::new(300.0, 150.0));
}

/// A flat view's rules, saved or made: its body is 1 to 255 bytes with no control characters, its turn
/// a quarter turn, it has a scale and is placed at a point, and no view of any kind is made from it yet.
#[test]
fn a_flat_view_keeps_its_rules() {
    let (d, mut id_source, id) = flat(QuarterTurn::None);
    let at = file(&d)["views"].as_array().unwrap().iter().position(|view| view["id"] == json!(ids::encode(id))).unwrap();
    let edits: [(fn(&mut Value), &str); 6] = [
        (|view| view["kind"]["flat"]["body"] = json!(""), "body is 1 to 255 bytes"),
        (|view| view["kind"]["flat"]["body"] = json!("Pl\u{7}ate"), "no control characters"),
        (|view| view["kind"]["flat"]["body"] = json!("x".repeat(256)), "body is 1 to 255 bytes"),
        (|view| view["kind"]["flat"]["turn"] = json!(45), "0, 90, 180 or 270"),
        (|view| view["settings"] = json!({}), "a flat pattern view has a scale"),
        (|view| view["place"] = json!({"aligned": 10.0}), "a flat pattern view is placed at a point"),
    ];
    for (edit, why) in edits {
        let error = refused(&d, |v| edit(&mut v["views"][at]));
        assert!(error.contains(why), "{why}: {error}");
    }
    let refusal = "not supported yet: a view made from a flat pattern view";
    assert_eq!(d.can_project_from(id).unwrap_err().to_string(), refusal);
    let at = ViewPlace::At([450.0, 150.0]);
    let kinds = [
        (projected(id, ProjectedSide::Right), ViewPlace::Aligned(450.0)),
        (section(id), at),
        (pictorial(id, Diagonal::AboveRight), at),
        (auxiliary(id, [1.0, 0.0]), at),
    ];
    for (kind, place) in kinds {
        let mut made_from = d.clone();
        add(&mut made_from, &mut id_source, kind, place, ViewOverrides::default());
        let error = made_from.validate().unwrap_err().to_string();
        assert!(error.contains(refusal), "{error}");
    }
}

/// A flat pattern turned in quarter turns maps its points exactly: the corner (100, 0) of a flat
/// lands at (100, 0), (0, 100), (-100, 0) and (0, -100), turned anticlockwise on paper, with no
/// rounding, and each basis is orthonormal.
#[test]
fn a_quarter_turn_maps_a_flat_exactly() {
    let want = [Vec2::new(100.0, 0.0), Vec2::new(0.0, 100.0), Vec2::new(-100.0, 0.0), Vec2::new(0.0, -100.0)];
    for (turn, want) in QuarterTurn::ALL.into_iter().zip(want) {
        let basis = turn.basis();
        solidcraft_geom::section::validate_plane(&basis).unwrap();
        assert_eq!(basis.to_local(Vec3::new(100.0, 0.0, 0.0)), want, "{turn:?}");
    }
}

/// A turn is saved as its degrees, and read only as 0, 90, 180 or 270.
#[test]
fn a_quarter_turn_is_saved_as_its_degrees() {
    for (turn, degrees) in QuarterTurn::ALL.into_iter().zip([0, 90, 180, 270]) {
        assert_eq!(serde_json::to_value(turn).unwrap(), json!(degrees));
        assert_eq!(serde_json::from_value::<QuarterTurn>(json!(degrees)).unwrap(), turn);
    }
    for bad in [json!(45), json!(-90), json!(360), json!("90"), json!(90.5), json!(null)] {
        assert!(serde_json::from_value::<QuarterTurn>(bad.clone()).is_err(), "{bad}");
    }
}

/// A view's hidden bodies (#22107) save in its settings as `{path, body}`, a left-out `path` reading
/// as `[]`, follow down its chain until a view sets its own (`[]` hides none), and read back as the
/// same bytes. A drawing saved before them reads unchanged and saves no `hide_bodies`. Their limits
/// are kept on reading, at each side of each.
#[test]
fn hidden_bodies_follow_their_chain_and_keep_their_limits() {
    let (mut d, _, [front, right, top]) = three();
    let older = d.to_json().unwrap();
    assert!(!older.contains("hide_bodies"));
    assert_eq!(Drawing::from_json(&older).unwrap().to_json().unwrap(), older);
    let none = d.resolve_view(top).unwrap();
    assert_eq!((none.hide_bodies.value, none.hide_bodies.set_by), (None, SetBy::Default));
    let bodies: HiddenBodies = vec![HiddenBody { path: vec![], body: "Door".into() }, HiddenBody { path: vec![7, 8], body: "Shelf".into() }].into();
    view_mut(&mut d, front).settings.hide_bodies = Some(bodies.clone());
    d.validate().unwrap();
    let followed = d.resolve_view(top).unwrap();
    assert_eq!((followed.hide_bodies.value, followed.hide_bodies.set_by), (Some(bodies.clone()), SetBy::Parent(front)));
    view_mut(&mut d, right).settings.hide_bodies = Some(Vec::new().into());
    let own = d.resolve_view(top).unwrap();
    assert_eq!((own.hide_bodies.value.as_deref().map(<[HiddenBody]>::len), own.hide_bodies.set_by), (Some(0), SetBy::Parent(right)));
    let v = file(&d);
    let at = v["views"].as_array().unwrap().iter().position(|view| view["id"] == json!(ids::encode(front))).unwrap();
    assert_eq!(v["views"][at]["settings"]["hide_bodies"], json!([{"path": [], "body": "Door"}, {"path": [7, 8], "body": "Shelf"}]));
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), d.to_json().unwrap());

    let with = |bodies: Value| refused(&d, |v| v["views"][at]["settings"]["hide_bodies"] = bodies);
    let many = |n: usize, ids: usize| json!(vec![json!({"path": vec![1u64; ids], "body": "B"}); n]);
    let cases = [
        (json!([{"body": "Door"}]), "read"),
        (many(1000, 4), "read"),
        (many(1001, 0), "at most 1000 bodies in hide_bodies"),
        (many(64, 64), "read"),
        (json!([vec![json!({"path": vec![1u64; 64], "body": "B"}); 64], vec![json!({"path": [1], "body": "B"})]].concat()), "and 4096 ids in all"),
        (json!([{"path": vec![1u64; MAX_PLACEMENT_DEPTH + 1], "body": "B"}]), "a hide_bodies path is 0..64 ids"),
        (json!([{"body": ""}]), "named in 1 to 128 characters"),
        (json!([{"body": "B".repeat(MAX_BODY_NAME)}]), "read"),
        (json!([{"body": "B".repeat(MAX_BODY_NAME + 1)}]), "named in 1 to 128 characters"),
        (json!([{"body": "B", "side": 1}]), "unknown field `side`"),
    ];
    for (bodies, why) in cases {
        let got = with(bodies.clone());
        assert!(got.contains(why), "{bodies}: {got}");
    }
}
