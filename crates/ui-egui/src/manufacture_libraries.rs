//! Machine / Post / Template Library browsers with linked git remotes.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::library::{CatalogEntry, DEFAULT_CAM_LIBRARY_REMOTE, LibraryLocation, LibraryRemote};

use crate::SolveApp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryDialogKind {
    Machine,
    Post,
    Template,
}

impl LibraryDialogKind {
    fn title(self) -> &'static str {
        match self {
            LibraryDialogKind::Machine => "Machine Library",
            LibraryDialogKind::Post => "Post Library",
            LibraryDialogKind::Template => "Template Library",
        }
    }

    fn api_kind(self) -> &'static str {
        match self {
            LibraryDialogKind::Machine => "machine",
            LibraryDialogKind::Post => "post",
            LibraryDialogKind::Template => "template",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarSel {
    Document,
    Local,
    LinkedAll,
    Remote(usize),
}

#[derive(Clone, Debug)]
pub struct LibraryDialogState {
    pub kind: LibraryDialogKind,
    pub search: String,
    pub sidebar: SidebarSel,
    pub remotes: Vec<LibraryRemote>,
    pub entries: Vec<CatalogEntry>,
    pub selected: Option<usize>,
    pub remote_url: String,
    pub remote_name: String,
    pub status: String,
    pub filter_cap: String,
}

impl LibraryDialogState {
    fn new(kind: LibraryDialogKind) -> Self {
        Self {
            kind,
            search: String::new(),
            sidebar: SidebarSel::Local,
            remotes: Vec::new(),
            entries: Vec::new(),
            selected: None,
            remote_url: DEFAULT_CAM_LIBRARY_REMOTE.into(),
            remote_name: "SolidCraft CAM Libraries".into(),
            status: String::new(),
            filter_cap: String::new(),
        }
    }
}

pub fn open(app: &mut SolveApp, kind: LibraryDialogKind) {
    let mut st = LibraryDialogState::new(kind);
    refresh(app, &mut st);
    app.manufacture_workspace.library_dialog = Some(st);
    app.manufacture_workspace.manage_stub_dialog = None;
}

fn refresh(app: &mut SolveApp, st: &mut LibraryDialogState) {
    let _ = app.run("manufacture.library.ensure_default_remote", json!({}));
    match app.run("manufacture.library.list", json!({"kind": st.kind.api_kind()})) {
        Ok(v) => {
            st.remotes = serde_json::from_value(v.get("remotes").cloned().unwrap_or(json!([]))).unwrap_or_default();
            st.entries = serde_json::from_value(v.get("entries").cloned().unwrap_or(json!([]))).unwrap_or_default();
            st.status.clear();
        }
        Err(e) => st.status = e,
    }
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut st) = app.manufacture_workspace.library_dialog.clone() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    let mut do_refresh = false;
    let mut do_sync: Option<Option<String>> = None;
    let mut do_add = false;
    let mut do_install: Option<String> = None;
    let mut do_remove: Option<String> = None;

    // Stay inside the harness screen (1000 px tall) and under WINDOW_WIDE_MAX_W: a centred
    // 480-tall window still clipped the top once content grew, and the column mins pushed past 760.
    let screen = ui.ctx().content_rect();
    let max_h = (screen.height() - 96.0).max(200.0);
    crate::frame::window(ui.ctx(), st.kind.title(), crate::frame::Width::Wide)
        .id(egui::Id::new("mfg_library_dialog").with(st.kind.title()))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .default_size([700.0, 400.0])
        .max_height(max_h)
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                ui.label("Search");
                ui.add(egui::TextEdit::singleline(&mut st.search).desired_width(180.0));
                if ui.button("Refresh").clicked() {
                    do_refresh = true;
                }
                if ui.button("Sync linked").clicked() {
                    do_sync = Some(None);
                }
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                // ── Left: sources ───────────────────────────────────────────
                ui.vertical(|ui| {
                    ui.set_min_width(120.0);
                    ui.set_max_width(140.0);
                    ui.label(RichText::new("Sources").strong());
                    if ui.selectable_label(st.sidebar == SidebarSel::Document, "Document").clicked() {
                        st.sidebar = SidebarSel::Document;
                        st.selected = None;
                    }
                    if ui.selectable_label(st.sidebar == SidebarSel::Local, "Local").clicked() {
                        st.sidebar = SidebarSel::Local;
                        st.selected = None;
                    }
                    ui.label(RichText::new("Linked").small().strong());
                    if ui.selectable_label(st.sidebar == SidebarSel::LinkedAll, "All remotes").clicked() {
                        st.sidebar = SidebarSel::LinkedAll;
                        st.selected = None;
                    }
                    for (i, r) in st.remotes.iter().enumerate() {
                        let label = if r.enabled { r.name.clone() } else { format!("{} (off)", r.name) };
                        if ui.selectable_label(st.sidebar == SidebarSel::Remote(i), label).clicked() {
                            st.sidebar = SidebarSel::Remote(i);
                            st.selected = None;
                        }
                        ui.small(RichText::new(&r.url).weak());
                    }
                    ui.separator();
                    ui.label(RichText::new("Add remote").strong());
                    ui.label("Git URL or local folder");
                    ui.add(egui::TextEdit::singleline(&mut st.remote_url).desired_width(140.0));
                    ui.add(egui::TextEdit::singleline(&mut st.remote_name).desired_width(140.0));
                    if ui.button("Link repository").clicked() {
                        do_add = true;
                    }
                    ui.label(RichText::new("Central catalog holds machine-specific posts, machines, and templates.").small().weak());
                });
                ui.separator();
                // ── Center: list ────────────────────────────────────────────
                ui.vertical(|ui| {
                    ui.set_min_width(280.0);
                    ui.set_max_width(360.0);
                    let filtered: Vec<(usize, &CatalogEntry)> = st
                        .entries
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| match st.sidebar {
                            SidebarSel::Document => e.location == LibraryLocation::Document,
                            SidebarSel::Local => e.location == LibraryLocation::Local,
                            SidebarSel::LinkedAll => e.location == LibraryLocation::Linked,
                            SidebarSel::Remote(i) => e.location == LibraryLocation::Linked && st.remotes.get(i).is_some_and(|r| r.id == e.source_id),
                        })
                        .filter(|(_, e)| {
                            if st.search.is_empty() {
                                return true;
                            }
                            let q = st.search.to_ascii_lowercase();
                            e.name.to_ascii_lowercase().contains(&q)
                                || e.vendor.to_ascii_lowercase().contains(&q)
                                || e.description.to_ascii_lowercase().contains(&q)
                        })
                        .filter(|(_, e)| {
                            if st.filter_cap.is_empty() {
                                return true;
                            }
                            e.capabilities.iter().any(|c| c.eq_ignore_ascii_case(&st.filter_cap))
                        })
                        .collect();
                    ui.label(format!("{} items", filtered.len()));
                    egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                        for (idx, e) in filtered {
                            let sel = st.selected == Some(idx);
                            let row = format!("{}  ·  {}  ·  {}", e.name, if e.vendor.is_empty() { "—" } else { &e.vendor }, e.location.label());
                            if ui.selectable_label(sel, row).clicked() {
                                st.selected = Some(idx);
                            }
                            if !e.description.is_empty() {
                                ui.small(RichText::new(&e.description).weak());
                            }
                        }
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Install into document").clicked()
                            && let Some(i) = st.selected
                            && let Some(e) = st.entries.get(i)
                        {
                            do_install = Some(e.id.clone());
                        }
                        if let SidebarSel::Remote(i) = st.sidebar
                            && let Some(r) = st.remotes.get(i)
                        {
                            if ui.button("Sync this remote").clicked() {
                                do_sync = Some(Some(r.id.clone()));
                            }
                            if ui.button("Unlink").clicked() {
                                do_remove = Some(r.id.clone());
                            }
                        }
                    });
                });
                ui.separator();
                // ── Right: filters / info ───────────────────────────────────
                ui.vertical(|ui| {
                    ui.set_min_width(120.0);
                    ui.set_max_width(140.0);
                    ui.label(RichText::new("Filters").strong());
                    ui.label("Capability");
                    for cap in ["", "milling", "turning", "additive", "fabrication", "inspection", "holes"] {
                        let label = if cap.is_empty() { "All" } else { cap };
                        if ui.selectable_label(st.filter_cap == cap, label).clicked() {
                            st.filter_cap = cap.into();
                        }
                    }
                    ui.separator();
                    ui.label(RichText::new("Info").strong());
                    if let Some(i) = st.selected {
                        if let Some(e) = st.entries.get(i) {
                            ui.label(&e.name);
                            if !e.vendor.is_empty() {
                                ui.label(format!("Vendor: {}", e.vendor));
                            }
                            ui.label(format!("Location: {}", e.location.label()));
                            if !e.source_id.is_empty() {
                                ui.label(format!("Remote: {}", e.source_id));
                            }
                            if !e.path.is_empty() {
                                ui.small(&e.path);
                            }
                            if !e.capabilities.is_empty() {
                                ui.label(format!("Caps: {}", e.capabilities.join(", ")));
                            }
                            ui.label(&e.description);
                        }
                    } else {
                        ui.label(RichText::new("Select an entry").weak());
                    }
                    ui.separator();
                    ui.label(RichText::new(format!("Default remote:\n{DEFAULT_CAM_LIBRARY_REMOTE}")).small().weak());
                });
            });
            if !st.status.is_empty() {
                ui.colored_label(egui::Color32::from_rgb(200, 80, 80), &st.status);
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
        });

    if !open || close {
        app.manufacture_workspace.library_dialog = None;
        return;
    }
    if do_refresh {
        refresh(app, &mut st);
    }
    if do_add {
        let mut params = json!({"url": st.remote_url.trim()});
        if !st.remote_name.trim().is_empty() {
            params["name"] = json!(st.remote_name.trim());
        }
        match app.run("manufacture.library.add_remote", params) {
            Ok(_) => {
                st.status = "Remote linked".into();
                refresh(app, &mut st);
            }
            Err(e) => st.status = e,
        }
    }
    if let Some(id_opt) = do_sync {
        let params = match id_opt {
            Some(id) => json!({"id": id}),
            None => json!({}),
        };
        match app.run("manufacture.library.sync", params) {
            Ok(v) => {
                st.status = format!("Sync: {v}");
                refresh(app, &mut st);
            }
            Err(e) => st.status = e,
        }
    }
    if let Some(id) = do_remove {
        match app.run("manufacture.library.remove_remote", json!({"id": id})) {
            Ok(_) => {
                st.sidebar = SidebarSel::LinkedAll;
                refresh(app, &mut st);
            }
            Err(e) => st.status = e,
        }
    }
    if let Some(entry_id) = do_install {
        match app.run("manufacture.library.install", json!({"entry_id": entry_id})) {
            Ok(_) => {
                st.status = "Installed into document".into();
                app.manufacture_workspace.revision = u64::MAX;
                refresh(app, &mut st);
            }
            Err(e) => st.status = e,
        }
    }
    app.manufacture_workspace.library_dialog = Some(st);
}
