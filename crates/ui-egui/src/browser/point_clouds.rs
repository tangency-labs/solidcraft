//! The Point Clouds folder of the Browser (#5151): an eye per cloud and for the folder, a menu,
//! and the Edit Point Cloud card (the file's units, the size of the dots, how they are coloured:
//! the file's colours, one colour, or by height between two heights), whose live edits of one
//! opening are one undo step.

use serde_json::{Value, json};
use solidcraft_engine::doc::point_cloud::{ColorBy, PLAIN, PointCloud, UNITS, byte};
use solidcraft_engine::view::POINT_CLOUDS_FOLDER;

use super::{Actions, Row, draw_row, is_open, panel_pos, toggle, toggle_folder};
use crate::SolveApp;
use crate::context_menu::{Item, Target};
use crate::ui_kit;

/// The Edit Point Cloud card: the cloud, the undo depth when it opened, and the automatic
/// height range of the points it was worked out for (it sorts a sample of them).
#[derive(Clone, Debug)]
pub struct CloudPanel {
    pub cloud: u64,
    pub(crate) depth: usize,
    auto: Option<(usize, [u64; 16], Option<[f64; 2]>)>,
}

impl CloudPanel {
    pub fn new(app: &SolveApp, cloud: u64) -> Self {
        CloudPanel { cloud, depth: app.session.undo.len(), auto: None }
    }
    /// The cloud's automatic height range, worked out again only when its points or placement
    /// change.
    fn auto_range(&mut self, c: &PointCloud) -> Option<[f64; 2]> {
        let key = (std::sync::Arc::as_ptr(c.points.values()).cast::<f32>() as usize, std::array::from_fn(|i| c.transform[i / 4][i % 4].to_bits()));
        match &self.auto {
            Some((p, t, r)) if (*p, *t) == key => *r,
            _ => {
                let r = c.auto_height_range();
                self.auto = Some((key.0, key.1, r));
                r
            }
        }
    }
}

/// The ways the card colours the points, as the dropdown lists them.
const COLOR_BY: [(ColorBy, &str); 3] = [(ColorBy::File, "File Colors"), (ColorBy::Single, "Single Color"), (ColorBy::Height, "Height")];

pub(super) fn rows(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize, acts: &mut Actions) {
    let clouds: Vec<(u64, String, bool)> = app.session.doc.point_clouds.iter().map(|c| (c.id, c.name.clone(), c.visible)).collect();
    if clouds.is_empty() {
        return;
    }
    let key = "point_clouds";
    let open = is_open(app, key, true);
    // The folder's eye is its own: each cloud keeps its eye while the folder hides them all.
    let shown = !app.session.folder_hidden(POINT_CLOUDS_FOLDER);
    let r = draw_row(
        ui,
        ui.id().with(key),
        &Row { depth, fold: Some(open), eye: Some(shown), icon: "folder", label: "Point Clouds", dim: !shown, ..Default::default() },
    );
    if r.fold || r.clicked {
        toggle(app, key, true);
    }
    if r.eye {
        toggle_folder(app, POINT_CLOUDS_FOLDER);
    }
    if !open {
        return;
    }
    for (id, name, visible) in clouds {
        let r = draw_row(
            ui,
            ui.id().with(("point_cloud", id)),
            &Row { depth: depth + 1, eye: Some(visible), icon: "point", label: &name, name: true, dim: !visible || !shown, ..Default::default() },
        );
        if r.eye {
            let _ = app.run("pointcloud.edit", json!({ "cloud": id, "visible": !visible }));
        }
        if r.double {
            app.tree.cloud_panel = Some(CloudPanel::new(app, id));
        }
        if let Some(p) = r.secondary {
            acts.menu = Some((p, Target::PointCloud { id }));
        }
    }
}

/// The menu of a point cloud.
pub fn items(app: &SolveApp, id: u64) -> Vec<Item> {
    let visible = app.session.doc.point_clouds.iter().find(|c| c.id == id).is_some_and(|c| c.visible);
    vec![
        Item::action("ui.editPointCloud", "Edit Point Cloud", "edit").with(json!({ "cloud": id })),
        Item::sep(),
        Item::action("pointcloud.delete", "Delete", "delete").with(json!({ "cloud": id })),
        Item::sep(),
        Item::action("pointcloud.edit", "Show/Hide", "eye").key("V").with(json!({ "cloud": id, "visible": !visible })),
    ]
}

pub(crate) fn panel(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut m) = app.tree.cloud_panel.clone() else { return };
    let Some(c) = app.session.doc.point_clouds.iter().find(|c| c.id == m.cloud).cloned() else {
        app.tree.cloud_panel = None;
        return;
    };
    let mut edit: Option<Value> = None;
    let title = crate::i18n::format("browser.edit_point_cloud_title", &[("name", &c.name)]);
    let (units, decimals) = (app.session.doc.units.clone(), app.preferences.length_decimals);
    let auto = if c.color_by == ColorBy::Height { m.auto_range(&c) } else { None };
    let at = panel_pos(app);
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_point_cloud_panel"), "point", &title)
        .placed(egui::Align2::LEFT_TOP, at)
        .fields()
        .actions("", crate::i18n::tr("ui.close"), None)
        .show(ctx, true, |ui| {
            ui_kit::row(ui, crate::i18n::tr_label("Points"), |ui| ui.label(c.len().to_string()));
            let names: Vec<&str> = UNITS.iter().map(|(u, _)| *u).collect();
            let was = names.iter().position(|u| *u == c.units).unwrap_or(0);
            let mut unit = was;
            // A dropdown's response doesn't report the pick: compare the index.
            ui_kit::row(ui, crate::i18n::tr("ui.point_cloud_units"), |ui| ui_kit::dropdown(ui, "point_cloud_units", &mut unit, &names));
            if unit != was
                && let Some(u) = names.get(unit)
            {
                edit = Some(json!({ "cloud": c.id, "units": u }));
            }
            let mut size = c.point_size;
            if ui_kit::row(ui, crate::i18n::tr("ui.point_cloud_size"), |ui| ui_kit::slider(ui, &mut size, 0.5..=16.0, Default::default())).changed() {
                edit = Some(json!({ "cloud": c.id, "point_size": size }));
            }
            let labels: Vec<&str> = COLOR_BY.iter().map(|(_, l)| *l).collect();
            let was = COLOR_BY.iter().position(|(b, _)| *b == c.color_by).unwrap_or(0);
            let mut by = was;
            ui_kit::row(ui, crate::i18n::tr("ui.point_cloud_color_by"), |ui| ui_kit::dropdown(ui, "point_cloud_color_by", &mut by, &labels));
            if by != was
                && let Some((b, _)) = COLOR_BY.get(by)
            {
                edit = Some(json!({ "cloud": c.id, "color_by": b.name() }));
            }
            match c.color_by {
                ColorBy::File if c.colors.is_empty() => {
                    ui_kit::row(ui, "", |ui| ui.weak(crate::i18n::tr("ui.point_cloud_no_file_colors")));
                }
                ColorBy::File => {}
                ColorBy::Single => {
                    let mut rgb = c.color.unwrap_or(PLAIN).map(byte);
                    if ui_kit::row(ui, crate::i18n::tr_label("Colour"), |ui| ui_kit::color_field(ui, &mut rgb)).changed() {
                        edit = Some(json!({ "cloud": c.id, "color": rgb.map(|v| f64::from(v) / 255.0) }));
                    }
                }
                ColorBy::Height => {
                    let automatic = c.height_range.is_none();
                    if ui_kit::checkbox_row(ui, "point_cloud_auto_range", crate::i18n::tr("ui.point_cloud_auto_range"), automatic) {
                        // Off: start from the range worked out, which the fields then show.
                        edit = Some(json!({ "cloud": c.id, "height_range": if automatic { json!(auto) } else { Value::Null } }));
                    }
                    if let Some([mut lo, mut hi]) = c.height_range.or(auto) {
                        let mut changed = false;
                        ui.add_enabled_ui(!automatic, |ui| {
                            changed |= ui_kit::row(ui, crate::i18n::tr("ui.point_cloud_height_low"), |ui| {
                                ui_kit::length_field(ui, &mut lo, &units, decimals)
                            })
                            .changed();
                            changed |= ui_kit::row(ui, crate::i18n::tr("ui.point_cloud_height_high"), |ui| {
                                ui_kit::length_field(ui, &mut hi, &units, decimals)
                            })
                            .changed();
                        });
                        // The two heights stay apart, lowest first.
                        if changed && !automatic {
                            let hi = hi.max(lo + 1e-3);
                            edit = Some(json!({ "cloud": c.id, "height_range": [lo, hi] }));
                        }
                    }
                }
            }
        });
    if let Some(p) = edit
        && app.run("pointcloud.edit", p).is_ok()
    {
        // One undo step for the whole edit.
        if app.session.undo.len() > m.depth + 1 {
            let keep = app.session.undo.get(m.depth).cloned();
            app.session.undo.truncate(m.depth);
            app.session.undo.extend(keep);
        }
    }
    app.tree.cloud_panel = if closed || action == ui_kit::Action::Cancel { None } else { Some(m) };
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn a_cloud_has_a_menu_and_a_card_that_closes() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let dir = std::env::temp_dir().join(format!("solidcraft-browser-cloud-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scan.xyz");
        std::fs::write(
            &path,
            "0 0 0 255 0 0
1 0 0 0 255 0
",
        )
        .unwrap();
        let id = h.app.run("pointcloud.insert", json!({"path": path.to_string_lossy()})).unwrap()["cloud"].as_u64().unwrap();
        let items = super::items(&h.app, id);
        assert!(items.iter().any(|i| i.id == "pointcloud.delete"));
        h.app.tree.cloud_panel = Some(super::CloudPanel::new(&h.app, id));
        h.frames(4);
        assert!(h.widget("Close").is_some());
        // Each way of colouring shows its own rows.
        for by in ["single", "height", "file"] {
            h.app.run("pointcloud.edit", json!({"cloud": id, "color_by": by})).unwrap();
            h.frames(2);
        }
        h.app.run("pointcloud.edit", json!({"cloud": id, "color_by": "height"})).unwrap();
        h.frames(2);
        assert!(h.widget(crate::i18n::tr("ui.point_cloud_auto_range")).is_some());
        h.step(&json!({"widget": "Close"})).unwrap();
        h.frames(2);
        assert!(h.app.tree.cloud_panel.is_none());
        assert_eq!(h.app.session.doc.point_clouds.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
