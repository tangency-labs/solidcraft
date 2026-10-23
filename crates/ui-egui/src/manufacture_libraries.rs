//! Machine / Post / Template Library browsers.
//! Document remotes persist through manufacture.library_remotes.set; not companion HTTP, git sync, or GPL CAM.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::library::{LibraryLocation, LibraryRemote};

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarSel {
    Document,
    Remote(usize),
}

#[derive(Clone, Debug)]
struct CatalogRow {
    id: String,
    name: String,
    vendor: String,
    description: String,
    location: LibraryLocation,
    source_id: String,
    path: String,
    capabilities: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct LibraryDialogState {
    pub kind: LibraryDialogKind,
    pub search: String,
    pub sidebar: SidebarSel,
    pub remotes: Vec<LibraryRemote>,
    entries: Vec<CatalogRow>,
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
            sidebar: SidebarSel::Document,
            remotes: Vec::new(),
            entries: Vec::new(),
            selected: None,
            remote_url: String::new(),
            remote_name: "CAM Library".into(),
            status: String::new(),
            filter_cap: String::new(),
        }
    }
}

pub fn open(app: &mut SolveApp, kind: LibraryDialogKind) {
    crate::manufacture_workspace::ensure_setup(app);
    clear_other(app);
    let mut st = LibraryDialogState::new(kind);
    refresh(app, &mut st);
    app.manufacture_workspace.library_dialog = Some(st);
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Libraries;
}

pub fn open_from_command(app: &mut SolveApp, id: &str) -> bool {
    match id {
        "manufacture.manage.machine_library" => open(app, LibraryDialogKind::Machine),
        "manufacture.manage.post_library" => open(app, LibraryDialogKind::Post),
        "manufacture.manage.template_library" => open(app, LibraryDialogKind::Template),
        _ => return true,
    }
    false
}

fn clear_other(app: &mut SolveApp) {
    app.manufacture_workspace.tool_library_dialog = None;
    app.manufacture_workspace.edit_tool_dialog = None;
    app.manufacture_workspace.manage_stub_dialog = None;
    app.manufacture_workspace.setup_dialog = None;
    app.manufacture_workspace.drill_dialog = None;
    app.manufacture_workspace.contour_dialog = None;
    app.manufacture_workspace.twod_dialog = None;
    app.manufacture_workspace.threed_dialog = None;
    app.manufacture_workspace.hole_rec_dialog = None;
    app.manufacture_workspace.multiaxis_dialog = None;
    app.manufacture_workspace.turning_dialog = None;
    app.manufacture_workspace.part_handling_dialog = None;
    app.manufacture_workspace.inspection_dialog = None;
    app.manufacture_workspace.manual_inspection_dialog = None;
    app.manufacture_workspace.fabrication_dialog = None;
    app.manufacture_workspace.nest_dialog = None;
    app.manufacture_workspace.additive_dialog = None;
    app.manufacture_workspace.from_gcode_dialog = None;
    app.manufacture_workspace.modify_dialog = None;
    app.manufacture_workspace.post_dialog = None;
    app.manufacture_workspace.machining_time_dialog = None;
    app.manufacture_workspace.setup_sheet_dialog = None;
    app.manufacture_workspace.library_dialog = None;
}

fn refresh(app: &SolveApp, st: &mut LibraryDialogState) {
    st.remotes = app.session.doc.manufacture.library_remotes.clone();
    st.entries = document_entries(app, st.kind);
    st.selected = None;
}

fn document_entries(app: &SolveApp, kind: LibraryDialogKind) -> Vec<CatalogRow> {
    let m = &app.session.doc.manufacture;
    let mut rows = Vec::new();
    match kind {
        LibraryDialogKind::Machine => {
            for (id, machine) in &m.machines {
                let name = if machine.name.trim().is_empty() { id.clone() } else { machine.name.clone() };
                rows.push(CatalogRow {
                    id: format!("document/machine/{id}"),
                    name,
                    vendor: machine.oem.clone(),
                    description: machine.model.clone(),
                    location: LibraryLocation::Document,
                    source_id: String::new(),
                    path: id.clone(),
                    capabilities: Vec::new(),
                });
            }
        }
        LibraryDialogKind::Post => {
            for (id, post) in &m.posts {
                let name = if post.name.trim().is_empty() { id.clone() } else { post.name.clone() };
                rows.push(CatalogRow {
                    id: format!("document/post/{id}"),
                    name,
                    vendor: String::new(),
                    description: format!("{:?}", post.dialect),
                    location: LibraryLocation::Document,
                    source_id: String::new(),
                    path: id.clone(),
                    capabilities: Vec::new(),
                });
            }
        }
        LibraryDialogKind::Template => {
            for (id, template) in &m.templates {
                rows.push(CatalogRow {
                    id: format!("document/template/{id}"),
                    name: template.name.clone(),
                    vendor: String::new(),
                    description: template.description.clone(),
                    location: LibraryLocation::Document,
                    source_id: String::new(),
                    path: id.clone(),
                    capabilities: if template.category.is_empty() { Vec::new() } else { vec![template.category.clone()] },
                });
            }
        }
    }
    rows
}

fn remote_id(name: &str, existing: &[LibraryRemote]) -> String {
    let base: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let base = base.trim_matches('-');
    let base = if base.is_empty() { "remote" } else { base };
    let mut id = base.to_string();
    let mut n = 2;
    while existing.iter().any(|r| r.id == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut st) = app.manufacture_workspace.library_dialog.clone() else {
        return;
    };
    let mut do_refresh = false;
    let mut do_add = false;
    let mut do_remove: Option<String> = None;
    let title = st.kind.title();
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("mfg_library_dialog"), "settings", title)
        .fields()
        .cards(2.0)
        .actions("", "Close", None)
        .show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                ui.label("Search");
                ui.add(egui::TextEdit::singleline(&mut st.search).desired_width(180.0));
                if ui.button("Refresh").clicked() {
                    do_refresh = true;
                }
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_min_width(140.0);
                    ui.label(RichText::new("Sources").strong());
                    if ui.selectable_label(st.sidebar == SidebarSel::Document, "Document").clicked() {
                        st.sidebar = SidebarSel::Document;
                        st.selected = None;
                    }
                    ui.label(RichText::new("Linked").small().strong());
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
                    if let SidebarSel::Remote(i) = st.sidebar {
                        if let Some(r) = st.remotes.get(i) {
                            if ui.button("Unlink").clicked() {
                                do_remove = Some(r.id.clone());
                            }
                        }
                    }
                    ui.label(RichText::new("Linked git sync lands in a follow-up PR. Remotes are stored on the document.").small().weak());
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_min_width(280.0);
                    let filtered: Vec<(usize, &CatalogRow)> = st
                        .entries
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| match st.sidebar {
                            SidebarSel::Document => e.location == LibraryLocation::Document,
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
                            let vendor = if e.vendor.is_empty() { "-" } else { e.vendor.as_str() };
                            let row = format!("{}  ·  {}  ·  {}", e.name, vendor, e.location.label());
                            if ui.selectable_label(sel, row).clicked() {
                                st.selected = Some(idx);
                            }
                            if !e.description.is_empty() {
                                ui.small(RichText::new(&e.description).weak());
                            }
                        }
                    });
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_min_width(140.0);
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
                            ui.small(&e.id);
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
                });
            });
            if !st.status.is_empty() {
                ui.colored_label(egui::Color32::from_rgb(200, 80, 80), &st.status);
            }
        });
    if closed || action == crate::ui_kit::Action::Cancel || action == crate::ui_kit::Action::Ok {
        app.manufacture_workspace.library_dialog = None;
        return;
    }
    if do_refresh {
        refresh(app, &mut st);
        st.status.clear();
    }
    if do_add {
        let url = st.remote_url.trim().to_string();
        let name = st.remote_name.trim().to_string();
        if url.is_empty() {
            st.status = app.tr("manufacture.library.need_url").to_string();
        } else {
            let mut remote = LibraryRemote::default();
            remote.id = remote_id(&name, &st.remotes);
            remote.name = if name.is_empty() { remote.id.clone() } else { name };
            remote.url = url;
            match serde_json::to_value(&remote) {
                Ok(v) => match app.run("manufacture.library_remotes.set", json!({"add": v})) {
                    Ok(_) => {
                        st.status = app.tr("manufacture.library.remote_linked").to_string();
                        refresh(app, &mut st);
                    }
                    Err(e) => st.status = e,
                },
                Err(e) => st.status = e.to_string(),
            }
        }
    }
    if let Some(id) = do_remove {
        match app.run("manufacture.library_remotes.set", json!({"remove": id})) {
            Ok(_) => {
                st.sidebar = SidebarSel::Document;
                st.status = app.tr("manufacture.library.remote_unlinked").to_string();
                refresh(app, &mut st);
            }
            Err(e) => st.status = e,
        }
    }
    app.manufacture_workspace.library_dialog = Some(st);
}
