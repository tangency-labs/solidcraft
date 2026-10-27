//! Tool Library + Edit Tool dialogs
//! (General / Cutter / Shaft / Holder / Cutting data / Post processor)
//! and MANAGE ribbon stubs (Form Mill, libraries, defaults).

use egui::RichText;
use serde_json::json;

// Tool Library types live here (not crates/manufacture). Persist through cam.tool.define.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ToolKind {
    #[default]
    FlatEndmill,
    BallEndmill,
    BullnoseEndmill,
    Drill,
    Chamfer,
    Facemill,
    Tap,
    Probe,
    Waterjet,
    Laser,
    Plasma,
}

impl ToolKind {
    pub const ALL: &'static [ToolKind] = &[
        ToolKind::FlatEndmill,
        ToolKind::BallEndmill,
        ToolKind::BullnoseEndmill,
        ToolKind::Drill,
        ToolKind::Chamfer,
        ToolKind::Facemill,
        ToolKind::Tap,
        ToolKind::Probe,
        ToolKind::Waterjet,
        ToolKind::Laser,
        ToolKind::Plasma,
    ];

    fn label(self) -> &'static str {
        match self {
            ToolKind::FlatEndmill => "Flat end mill",
            ToolKind::BallEndmill => "Ball end mill",
            ToolKind::BullnoseEndmill => "Bull nose end mill",
            ToolKind::Drill => "Drill",
            ToolKind::Chamfer => "Engrave/Chamfer mill",
            ToolKind::Facemill => "Face mill",
            ToolKind::Tap => "Tap",
            ToolKind::Probe => "Probe",
            ToolKind::Waterjet => "Waterjet",
            ToolKind::Laser => "Laser cutter",
            ToolKind::Plasma => "Plasma cutter",
        }
    }

    fn category(self) -> ToolCategory {
        match self {
            ToolKind::Drill | ToolKind::Tap => ToolCategory::HoleMaking,
            ToolKind::Probe => ToolCategory::Probe,
            ToolKind::Waterjet | ToolKind::Laser | ToolKind::Plasma => ToolCategory::Cutting,
            _ => ToolCategory::Milling,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ToolCategory {
    #[default]
    Milling,
    HoleMaking,
    Turning,
    Cutting,
    Probe,
    Holders,
}

impl ToolCategory {
    pub const ALL: &'static [ToolCategory] =
        &[ToolCategory::Milling, ToolCategory::HoleMaking, ToolCategory::Turning, ToolCategory::Cutting, ToolCategory::Probe, ToolCategory::Holders];

    fn label(self) -> &'static str {
        match self {
            ToolCategory::Milling => "Milling",
            ToolCategory::HoleMaking => "Hole making",
            ToolCategory::Turning => "Turning",
            ToolCategory::Cutting => "Cutting",
            ToolCategory::Probe => "Probe",
            ToolCategory::Holders => "Holders",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ToolMaterial {
    Hss,
    #[default]
    Carbide,
    Cobalt,
    Ceramic,
    Diamond,
    Unknown,
}

impl ToolMaterial {
    pub const ALL: &'static [ToolMaterial] =
        &[ToolMaterial::Hss, ToolMaterial::Carbide, ToolMaterial::Cobalt, ToolMaterial::Ceramic, ToolMaterial::Diamond, ToolMaterial::Unknown];

    fn label(self) -> &'static str {
        match self {
            ToolMaterial::Hss => "HSS",
            ToolMaterial::Carbide => "Carbide",
            ToolMaterial::Cobalt => "Cobalt",
            ToolMaterial::Ceramic => "Ceramic",
            ToolMaterial::Diamond => "Diamond",
            ToolMaterial::Unknown => "Unknown",
        }
    }

    fn param(self) -> &'static str {
        match self {
            ToolMaterial::Hss => "hss",
            ToolMaterial::Carbide => "carbide",
            ToolMaterial::Cobalt => "cobalt",
            ToolMaterial::Ceramic => "ceramic",
            ToolMaterial::Diamond => "diamond",
            ToolMaterial::Unknown => "unknown",
        }
    }

    fn from_param(s: &str) -> Self {
        match s {
            "hss" => ToolMaterial::Hss,
            "carbide" => ToolMaterial::Carbide,
            "cobalt" => ToolMaterial::Cobalt,
            "ceramic" => ToolMaterial::Ceramic,
            "diamond" => ToolMaterial::Diamond,
            _ => ToolMaterial::Unknown,
        }
    }

    fn youngs_gpa(self) -> f64 {
        match self {
            ToolMaterial::Carbide => 600.0,
            ToolMaterial::Ceramic => 300.0,
            ToolMaterial::Diamond => 1050.0,
            ToolMaterial::Hss | ToolMaterial::Cobalt | ToolMaterial::Unknown => 210.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Coolant {
    #[default]
    None,
    Flood,
    Mist,
    Air,
    ThroughSpindle,
}

impl Coolant {
    pub const ALL: &'static [Coolant] = &[Coolant::None, Coolant::Flood, Coolant::Mist, Coolant::Air, Coolant::ThroughSpindle];

    fn label(self) -> &'static str {
        match self {
            Coolant::None => "Disabled",
            Coolant::Flood => "Flood",
            Coolant::Mist => "Mist",
            Coolant::Air => "Air",
            Coolant::ThroughSpindle => "Through spindle",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NcUnit {
    #[default]
    Millimeters,
    Inches,
}

impl NcUnit {
    pub const ALL: &'static [NcUnit] = &[NcUnit::Millimeters, NcUnit::Inches];

    fn label(self) -> &'static str {
        match self {
            NcUnit::Millimeters => "Millimeters",
            NcUnit::Inches => "Inches",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ShaftSegment {
    pub height_mm: f64,
    pub upper_diameter_mm: f64,
    pub lower_diameter_mm: f64,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ToolHolder {
    pub description: String,
    pub product_id: String,
    pub product_link: String,
    pub vendor: String,
    pub gauge_length_mm: f64,
    /// Holder stiffness at the gauge face (N/µm). 20 is the #1621 placeholder until measured.
    pub stiffness_n_per_um: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CuttingPreset {
    pub name: String,
    pub spindle_rpm: u32,
    pub cutting_feedrate: f64,
    pub lead_in_feedrate: f64,
    pub lead_out_feedrate: f64,
    pub transition_feedrate: f64,
    pub ramp_feedrate: f64,
    pub ramp_angle_deg: f64,
    pub plunge_feedrate: f64,
    pub use_stepdown: bool,
    pub stepdown_mm: f64,
    pub use_stepover: bool,
    pub stepover_mm: f64,
    pub coolant: Coolant,
}

impl Default for CuttingPreset {
    fn default() -> Self {
        Self {
            name: "Default".into(),
            spindle_rpm: 12_000,
            cutting_feedrate: 500.0,
            lead_in_feedrate: 200.0,
            lead_out_feedrate: 200.0,
            transition_feedrate: 300.0,
            ramp_feedrate: 200.0,
            ramp_angle_deg: 3.0,
            plunge_feedrate: 200.0,
            use_stepdown: true,
            stepdown_mm: 1.0,
            use_stepover: true,
            stepover_mm: 2.0,
            coolant: Coolant::Air,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub number: u32,
    pub kind: ToolKind,
    pub diameter: f64,
    pub corner_radius: f64,
    pub length: f64,
    pub flute_length: f64,
    pub flutes: u8,
    pub description: String,
    pub vendor: String,
    pub product_id: String,
    pub product_link: String,
    pub unit: NcUnit,
    pub clockwise: bool,
    pub material: ToolMaterial,
    pub shaft_diameter: f64,
    pub length_below_holder: f64,
    pub shoulder_length: f64,
    pub gauge_length: f64,
    pub shaft_segments: Vec<ShaftSegment>,
    pub holder: Option<ToolHolder>,
    pub cutting_presets: Vec<CuttingPreset>,
    pub length_offset: u32,
    pub diameter_offset: u32,
    pub turret: u32,
    pub comment: String,
    pub manual_tool_change: bool,
    pub live_tool: bool,
    pub break_control: bool,
}

impl Default for Tool {
    fn default() -> Self {
        Self {
            number: 1,
            kind: ToolKind::FlatEndmill,
            diameter: 6.0,
            corner_radius: 0.0,
            length: 50.0,
            flute_length: 16.0,
            flutes: 4,
            description: String::new(),
            vendor: String::new(),
            product_id: String::new(),
            product_link: String::new(),
            unit: NcUnit::Millimeters,
            clockwise: true,
            material: ToolMaterial::Carbide,
            shaft_diameter: 6.0,
            length_below_holder: 30.0,
            shoulder_length: 20.0,
            gauge_length: 80.0,
            shaft_segments: Vec::new(),
            holder: None,
            cutting_presets: Vec::new(),
            length_offset: 1,
            diameter_offset: 1,
            turret: 0,
            comment: String::new(),
            manual_tool_change: false,
            live_tool: true,
            break_control: false,
        }
    }
}

impl Tool {
    pub fn display_name(&self) -> String {
        if self.description.is_empty() {
            format!("{} - ø{:.2}mm ({})", self.number, self.diameter, self.kind.label())
        } else {
            format!("{} - ø{:.2}mm ({})", self.number, self.diameter, self.description)
        }
    }

    pub fn surface_speed_m_min(&self, spindle_rpm: u32) -> f64 {
        std::f64::consts::PI * self.diameter * f64::from(spindle_rpm) / 1_000.0
    }

    pub fn feed_per_tooth(&self, feedrate: f64, spindle_rpm: u32) -> f64 {
        if spindle_rpm == 0 || self.flutes == 0 {
            0.0
        } else {
            feedrate / (f64::from(spindle_rpm) * f64::from(self.flutes))
        }
    }

    fn core_diameter(&self) -> f64 {
        if self.shaft_diameter > 0.0 { self.shaft_diameter } else { self.diameter * 0.8 }
    }

    fn youngs_gpa(&self) -> f64 {
        self.material.youngs_gpa()
    }

    /// Cantilever `k = 3EI/L³` in N/µm. Stick-out is length below holder.
    pub fn stiffness_n_per_um(&self) -> f64 {
        let l = self.length_below_holder.max(0.1);
        let d = self.core_diameter().max(0.1);
        let e = self.youngs_gpa() * 1_000.0;
        let i = std::f64::consts::PI / 64.0 * d.powi(4);
        (3.0 * e * i / l.powi(3)) / 1_000.0
    }

    pub fn assembly_stiffness_n_per_um(&self) -> f64 {
        let k_tool = self.stiffness_n_per_um();
        match self.holder.as_ref().map(|h| h.stiffness_n_per_um).filter(|k| *k > 0.0) {
            Some(k_holder) => 1.0 / (1.0 / k_tool.max(1e-9) + 1.0 / k_holder),
            None => k_tool,
        }
    }

    pub fn tooth_passing_hz(&self, spindle_rpm: u32) -> f64 {
        if spindle_rpm == 0 || self.flutes == 0 {
            0.0
        } else {
            f64::from(spindle_rpm) * f64::from(self.flutes) / 60.0
        }
    }
}

use crate::SolveApp;

// ── Edit Tool tabs ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EditToolTab {
    #[default]
    General,
    Cutter,
    Shaft,
    Holder,
    CuttingData,
    Physics,
    PostProcessor,
}

impl EditToolTab {
    const ALL: &'static [EditToolTab] = &[
        EditToolTab::General,
        EditToolTab::Cutter,
        EditToolTab::Shaft,
        EditToolTab::Holder,
        EditToolTab::CuttingData,
        EditToolTab::Physics,
        EditToolTab::PostProcessor,
    ];

    fn label(self) -> &'static str {
        match self {
            EditToolTab::General => "General",
            EditToolTab::Cutter => "Cutter",
            EditToolTab::Shaft => "Shaft",
            EditToolTab::Holder => "Holder",
            EditToolTab::CuttingData => "Cutting data",
            EditToolTab::Physics => "Physics",
            EditToolTab::PostProcessor => "Post processor",
        }
    }
}

#[derive(Clone, Debug)]
pub struct EditToolState {
    pub setup_name: String,
    /// `None` = append as new tool on OK.
    pub tool_index: Option<usize>,
    pub draft: Tool,
    pub tab: EditToolTab,
    pub selected_preset: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct ToolLibraryState {
    pub selected: Option<usize>,
    pub category: ToolCategory,
    pub search: String,
    pub info_tab: bool,
}

impl Default for ToolLibraryState {
    fn default() -> Self {
        Self { selected: None, category: ToolCategory::Milling, search: String::new(), info_tab: true }
    }
}

// ── MANAGE stubs ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManageStubKind {
    FormMill,
    SolidHolder,
    MachineLibrary,
    PostLibrary,
    TemplateLibrary,
    TaskManager,
    ExportDefaults,
    ImportDefaults,
    ResetDefaults,
}

impl ManageStubKind {
    fn title(self) -> &'static str {
        match self {
            ManageStubKind::FormMill => "Form Mill",
            ManageStubKind::SolidHolder => "Solid Holder",
            ManageStubKind::MachineLibrary => "Machine Library",
            ManageStubKind::PostLibrary => "Post Library",
            ManageStubKind::TemplateLibrary => "Template Library",
            ManageStubKind::TaskManager => "Task Manager",
            ManageStubKind::ExportDefaults => "Export Defaults",
            ManageStubKind::ImportDefaults => "Import Defaults",
            ManageStubKind::ResetDefaults => "Reset Defaults",
        }
    }

    fn body(self) -> &'static str {
        match self {
            ManageStubKind::FormMill => "Define a custom form-mill profile from model geometry. Coming in a later slice.",
            ManageStubKind::SolidHolder => "Create a tool holder from solid body geometry. Coming in a later slice.",
            ManageStubKind::MachineLibrary => "Browse and assign CNC machines. Use the Setup panel Machine field for now.",
            ManageStubKind::PostLibrary => "Browse post processors. Use Post Process (ACTIONS) or Setup → Post for now.",
            ManageStubKind::TemplateLibrary => "Save and reuse operation templates. Coming in a later slice.",
            ManageStubKind::TaskManager => "Background CAM generation tasks appear here. Use Generate status for now.",
            ManageStubKind::ExportDefaults => "Export default CAM parameters to a file. Coming soon.",
            ManageStubKind::ImportDefaults => "Import default CAM parameters from a file. Coming soon.",
            ManageStubKind::ResetDefaults => "Reset CAM defaults to factory values. Coming soon.",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ManageStubState {
    pub kind: ManageStubKind,
}

// ── Open helpers ──────────────────────────────────────────────────────────────

pub fn open_tool_library(app: &mut SolveApp) {
    crate::manufacture_workspace::enter(app);
    app.manufacture_workspace.tool_library_dialog =
        Some(ToolLibraryState { selected: app.manufacture_workspace.selected_tool, ..Default::default() });
    app.set_status(app.tr("manufacture.ribbon.tools").to_string(), false);
}

pub fn open_edit_tool(app: &mut SolveApp, tool_index: Option<usize>) {
    crate::manufacture_workspace::enter(app);
    let setup_name = app.manufacture_workspace.active_setup.clone();
    let tools = ui_tools(app);
    let draft = if let Some(i) = tool_index {
        tools.get(i).cloned().unwrap_or_default()
    } else {
        let next = tools.iter().map(|t| t.number).max().unwrap_or(0).saturating_add(1).max(1);
        Tool { number: next, length_offset: next, diameter_offset: next, ..Tool::default() }
    };
    let selected_preset = if draft.cutting_presets.is_empty() { None } else { Some(0) };
    app.manufacture_workspace.edit_tool_dialog = Some(EditToolState { setup_name, tool_index, draft, tab: EditToolTab::General, selected_preset });
}

pub fn open_manage_stub(app: &mut SolveApp, kind: ManageStubKind) {
    app.manufacture_workspace.manage_stub_dialog = Some(ManageStubState { kind });
}

// ── Dialog host ───────────────────────────────────────────────────────────────

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    show_tool_library(app, ui);
    show_edit_tool(app, ui);
    show_manage_stub(app, ui);
}

fn show_manage_stub(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(st) = app.manufacture_workspace.manage_stub_dialog.clone() else {
        return;
    };
    let title = st.kind.title().to_string();
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("mfg_manage_stub"), "settings", &title)
        .fields()
        .actions("", "Close", None)
        .cards(1.0)
        .show(ui.ctx(), true, |ui| {
            ui.label(st.kind.body());
        });
    if closed || action == crate::ui_kit::Action::Cancel {
        app.manufacture_workspace.manage_stub_dialog = None;
    }
}

fn show_tool_library(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut st) = app.manufacture_workspace.tool_library_dialog.clone() else {
        return;
    };
    let setup_name = app.manufacture_workspace.active_setup.clone();
    let tools: Vec<Tool> = ui_tools(app);

    let mut add = false;
    let mut edit = false;
    let mut delete = false;
    let mut duplicate = false;

    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("mfg_tool_library"), "settings", "Tool Library")
        .fields()
        .actions("", "Close", None)
        .cards(1.8)
        .show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                // Left: library tree + search
                ui.vertical(|ui| {
                    ui.set_min_width(120.0);
                    ui.set_max_width(140.0);
                    ui.label(RichText::new("Current Document").strong());
                    ui.label(if setup_name.is_empty() { "(no setup)".into() } else { setup_name.clone() });
                    ui.separator();
                    ui.label("Search");
                    ui.text_edit_singleline(&mut st.search);
                });

                ui.separator();

                // Center: tool table + cutting data
                ui.vertical(|ui| {
                    ui.set_min_width(320.0);
                    ui.set_max_width(400.0);
                    ui.horizontal(|ui| {
                        if ui.button("+ Add").clicked() {
                            add = true;
                        }
                        let has = st.selected.is_some();
                        if ui.add_enabled(has, egui::Button::new("Edit")).clicked() {
                            edit = true;
                        }
                        if ui.add_enabled(has, egui::Button::new("Copy")).clicked() {
                            duplicate = true;
                        }
                        if ui.add_enabled(has, egui::Button::new("Delete")).clicked() {
                            delete = true;
                        }
                    });
                    ui.separator();

                    let filtered: Vec<(usize, &Tool)> =
                        tools.iter().enumerate().filter(|(_, t)| matches_filter(t, st.category, &st.search)).collect();

                    egui::ScrollArea::both().id_salt("mfg_tl_list").max_height(240.0).show(ui, |ui| {
                        egui::Grid::new("mfg_tl_grid").striped(true).num_columns(6).show(ui, |ui| {
                            ui.strong("Name");
                            ui.strong("Diameter");
                            ui.strong("Flute length");
                            ui.strong("Overall length");
                            ui.strong("Type");
                            ui.strong("Stiffness");
                            ui.end_row();
                            for (i, t) in &filtered {
                                let sel = st.selected == Some(*i);
                                if ui.selectable_label(sel, t.display_name()).clicked() {
                                    st.selected = Some(*i);
                                }
                                ui.label(format!("{:.3} mm", t.diameter));
                                ui.label(format!("{:.2} mm", t.flute_length));
                                ui.label(format!("{:.1} mm", t.length));
                                ui.label(t.kind.label());
                                ui.label(format!("{:.1} N/µm", t.assembly_stiffness_n_per_um()));
                                ui.end_row();
                            }
                        });
                    });

                    ui.separator();
                    ui.label(RichText::new("Cutting data").strong());
                    if let Some(i) = st.selected {
                        if let Some(t) = tools.get(i) {
                            if t.cutting_presets.is_empty() {
                                ui.label("No cutting presets — Edit tool to add.");
                            } else {
                                egui::Grid::new("mfg_tl_presets").striped(true).num_columns(5).show(ui, |ui| {
                                    ui.strong("Cutting data");
                                    ui.strong("Spindle");
                                    ui.strong("Surface");
                                    ui.strong("Feedrate");
                                    ui.strong("Coolant");
                                    ui.end_row();
                                    for p in &t.cutting_presets {
                                        ui.label(&p.name);
                                        ui.label(format!("{} rpm", p.spindle_rpm));
                                        ui.label(format!("{:.3} m/min", t.surface_speed_m_min(p.spindle_rpm)));
                                        ui.label(format!("{:.0} mm/min", p.cutting_feedrate));
                                        ui.label(p.coolant.label());
                                        ui.end_row();
                                    }
                                });
                            }
                        }
                    } else {
                        ui.label("Select a tool");
                    }
                });

                ui.separator();

                // Right: Filters / Info
                ui.vertical(|ui| {
                    ui.set_min_width(140.0);
                    ui.set_max_width(160.0);
                    ui.horizontal(|ui| {
                        if ui.selectable_label(!st.info_tab, "Filters").clicked() {
                            st.info_tab = false;
                        }
                        if ui.selectable_label(st.info_tab, "Info").clicked() {
                            st.info_tab = true;
                        }
                    });
                    ui.separator();
                    if st.info_tab {
                        if let Some(i) = st.selected {
                            if let Some(t) = tools.get(i) {
                                ui.label(RichText::new(t.display_name()).strong());
                                tool_preview(ui, t);
                                ui.label(format!("Description: {}", t.description));
                                ui.label(format!("Vendor: {}", t.vendor));
                                ui.label(format!("Product id: {}", t.product_id));
                                ui.label(format!("Diameter: {:.3} mm", t.diameter));
                                ui.label(format!("Shaft diameter: {:.3} mm", t.shaft_diameter));
                                ui.label(format!("Overall length: {:.1} mm", t.length));
                                ui.label(format!("Length below holder: {:.2} mm", t.length_below_holder));
                                ui.label(format!("Shoulder length: {:.2} mm", t.shoulder_length));
                                ui.label(format!("Flute length: {:.2} mm", t.flute_length));
                                ui.label(format!("Estimated tool stiffness: {:.2} N/µm", t.stiffness_n_per_um()));
                                ui.label(format!("Assembly stiffness: {:.2} N/µm", t.assembly_stiffness_n_per_um()));
                            }
                        } else {
                            ui.label("Select a tool");
                        }
                    } else {
                        ui.label(RichText::new("Tool category").strong());
                        for cat in ToolCategory::ALL {
                            if ui.selectable_label(st.category == *cat, cat.label()).clicked() {
                                st.category = *cat;
                            }
                        }
                    }
                });
            });
        });

    if closed || action == crate::ui_kit::Action::Cancel {
        app.manufacture_workspace.tool_library_dialog = None;
        return;
    }

    if add {
        open_edit_tool(app, None);
    }
    if edit && let Some(i) = st.selected {
        open_edit_tool(app, Some(i));
    }
    if duplicate
        && let Some(i) = st.selected
        && let Some(src) = tools.get(i).cloned()
    {
        let mut tools = tools.clone();
        let next = tools.iter().map(|t| t.number).max().unwrap_or(0).saturating_add(1).max(1);
        let mut copy = src;
        copy.number = next;
        copy.length_offset = next;
        copy.diameter_offset = next;
        if !copy.description.is_empty() {
            copy.description = format!("{} (copy)", copy.description);
        }
        tools.push(copy);
        let new_idx = tools.len() - 1;
        if save_tools(app, &setup_name, tools) {
            st.selected = Some(new_idx);
            app.manufacture_workspace.selected_tool = Some(new_idx);
        }
    }
    if delete && let Some(i) = st.selected {
        let mut tools = tools.clone();
        if i < tools.len() {
            tools.remove(i);
            let next = if tools.is_empty() { None } else { Some(i.min(tools.len() - 1)) };
            if save_tools(app, &setup_name, tools) {
                st.selected = next;
                app.manufacture_workspace.selected_tool = next;
            }
        }
    }

    app.manufacture_workspace.selected_tool = st.selected;
    app.manufacture_workspace.tool_library_dialog = Some(st);
}

fn matches_filter(t: &Tool, category: ToolCategory, search: &str) -> bool {
    let cat_ok = match category {
        ToolCategory::Holders => t.holder.is_some(),
        ToolCategory::Turning | ToolCategory::Cutting => false,
        other => t.kind.category() == other,
    };
    if !cat_ok {
        return false;
    }
    if search.is_empty() {
        return true;
    }
    let q = search.to_ascii_lowercase();
    t.display_name().to_ascii_lowercase().contains(&q)
        || t.description.to_ascii_lowercase().contains(&q)
        || t.vendor.to_ascii_lowercase().contains(&q)
        || t.kind.label().to_ascii_lowercase().contains(&q)
}

fn show_edit_tool(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut st) = app.manufacture_workspace.edit_tool_dialog.clone() else {
        return;
    };

    let title = format!("Tool Library — {}", st.draft.display_name());
    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_edit_tool"), "settings", &title).fields().cards(1.8).show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                for tab in EditToolTab::ALL {
                    if ui.selectable_label(st.tab == *tab, tab.label()).clicked() {
                        st.tab = *tab;
                    }
                }
            });
            ui.separator();

            ui.horizontal(|ui| {
                egui::ScrollArea::vertical().id_salt("mfg_edit_tool_body").max_height(400.0).show(ui, |ui| {
                    ui.set_min_width(360.0);
                    match st.tab {
                        EditToolTab::General => tab_general(ui, &mut st.draft),
                        EditToolTab::Cutter => tab_cutter(ui, &mut st.draft),
                        EditToolTab::Shaft => tab_shaft(ui, &mut st.draft),
                        EditToolTab::Holder => tab_holder(ui, &mut st.draft),
                        EditToolTab::CuttingData => {
                            tab_cutting_data(ui, &mut st.draft, &mut st.selected_preset);
                        }
                        EditToolTab::Physics => tab_physics(ui, &mut st.draft),
                        EditToolTab::PostProcessor => tab_post(ui, &mut st.draft),
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_min_width(220.0);
                    ui.label(RichText::new("Preview").strong());
                    tool_preview(ui, &st.draft);
                });
            });
        });

    if closed || action == crate::ui_kit::Action::Cancel {
        app.manufacture_workspace.edit_tool_dialog = None;
        return;
    }
    if action == crate::ui_kit::Action::Ok && save_edit(app, &st) {
        app.manufacture_workspace.edit_tool_dialog = None;
        return;
    }
    app.manufacture_workspace.edit_tool_dialog = Some(st);
}

fn tab_general(ui: &mut egui::Ui, t: &mut Tool) {
    labeled_text(ui, "Description", &mut t.description);
    labeled_text(ui, "Vendor", &mut t.vendor);
    labeled_text(ui, "Product id", &mut t.product_id);
    labeled_text(ui, "Product link", &mut t.product_link);
}

fn tab_cutter(ui: &mut egui::Ui, t: &mut Tool) {
    ui.horizontal(|ui| {
        ui.label("Type");
        egui::ComboBox::from_id_salt("edit_tool_kind").selected_text(t.kind.label()).show_ui(ui, |ui| {
            for k in ToolKind::ALL {
                if ui.selectable_label(t.kind == *k, k.label()).clicked() {
                    t.kind = *k;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Unit");
        egui::ComboBox::from_id_salt("edit_tool_unit").selected_text(t.unit.label()).show_ui(ui, |ui| {
            for u in NcUnit::ALL {
                if ui.selectable_label(t.unit == *u, u.label()).clicked() {
                    t.unit = *u;
                }
            }
        });
    });
    ui.checkbox(&mut t.clockwise, "Clockwise spindle rotation");
    ui.horizontal(|ui| {
        ui.label("Number of flutes");
        let mut flutes = u32::from(t.flutes);
        if ui.add(egui::DragValue::new(&mut flutes).speed(1).range(1..=64)).changed() {
            t.flutes = flutes as u8;
        }
    });
    ui.horizontal(|ui| {
        ui.label("Material");
        egui::ComboBox::from_id_salt("edit_tool_mat").selected_text(t.material.label()).show_ui(ui, |ui| {
            for m in ToolMaterial::ALL {
                if ui.selectable_label(t.material == *m, m.label()).clicked() {
                    t.material = *m;
                }
            }
        });
    });
    ui.separator();
    ui.label(RichText::new("Geometry").strong());
    drag_mm(ui, "Diameter", &mut t.diameter, 0.01, 0.1..=500.0);
    drag_mm(ui, "Shaft diameter", &mut t.shaft_diameter, 0.01, 0.0..=500.0);
    drag_mm(ui, "Overall length", &mut t.length, 0.1, 0.1..=1000.0);
    drag_mm(ui, "Length below holder", &mut t.length_below_holder, 0.1, 0.0..=1000.0);
    drag_mm(ui, "Shoulder length", &mut t.shoulder_length, 0.1, 0.0..=1000.0);
    drag_mm(ui, "Flute length", &mut t.flute_length, 0.1, 0.1..=1000.0);
    if matches!(t.kind, ToolKind::BallEndmill | ToolKind::BullnoseEndmill) {
        drag_mm(ui, "Corner radius", &mut t.corner_radius, 0.01, 0.0..=250.0);
    }
    ui.separator();
    ui.label(RichText::new("Tool assembly").strong());
    drag_mm(ui, "Gauge length", &mut t.gauge_length, 0.1, 0.0..=2000.0);
}

fn tab_shaft(ui: &mut egui::Ui, t: &mut Tool) {
    ui.horizontal(|ui| {
        if ui.button("+ Add segment").clicked() {
            let d = if t.shaft_diameter > 0.0 { t.shaft_diameter } else { t.diameter };
            t.shaft_segments.push(ShaftSegment { height_mm: 10.0, upper_diameter_mm: d, lower_diameter_mm: d });
        }
        if ui.button("Clear").clicked() {
            t.shaft_segments.clear();
        }
    });
    ui.separator();
    let mut remove: Option<usize> = None;
    egui::Grid::new("shaft_segs").striped(true).num_columns(5).show(ui, |ui| {
        ui.strong("#");
        ui.strong("Height");
        ui.strong("Upper Ø");
        ui.strong("Lower Ø");
        ui.strong("");
        ui.end_row();
        for (i, seg) in t.shaft_segments.iter_mut().enumerate() {
            ui.label(format!("{}", i + 1));
            ui.add(egui::DragValue::new(&mut seg.height_mm).speed(0.1).suffix(" mm").range(0.01..=500.0));
            ui.add(egui::DragValue::new(&mut seg.upper_diameter_mm).speed(0.01).suffix(" mm").range(0.01..=500.0));
            ui.add(egui::DragValue::new(&mut seg.lower_diameter_mm).speed(0.01).suffix(" mm").range(0.01..=500.0));
            if ui.small_button("✕").clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        t.shaft_segments.remove(i);
    }
    if t.shaft_segments.is_empty() {
        ui.label("No shaft segments — cutter uses shaft diameter only.");
    }
}

fn tab_holder(ui: &mut egui::Ui, t: &mut Tool) {
    ui.horizontal(|ui| {
        if ui.button("Select holder").clicked() {
            t.holder = Some(ToolHolder {
                description: "Generic ER collet".into(),
                product_id: "ER32".into(),
                product_link: String::new(),
                vendor: "Generic".into(),
                gauge_length_mm: t.gauge_length.max(60.0),
                stiffness_n_per_um: 20.0,
            });
        }
        if ui.add_enabled(t.holder.is_some(), egui::Button::new("Remove holder")).clicked() {
            t.holder = None;
        }
    });
    ui.separator();
    if let Some(h) = t.holder.as_mut() {
        labeled_text(ui, "Description", &mut h.description);
        labeled_text(ui, "Product id", &mut h.product_id);
        labeled_text(ui, "Product link", &mut h.product_link);
        labeled_text(ui, "Vendor", &mut h.vendor);
        drag_mm(ui, "Gauge length", &mut h.gauge_length_mm, 0.1, 0.0..=2000.0);
        ui.horizontal(|ui| {
            ui.label("Holder stiffness");
            ui.add(egui::DragValue::new(&mut h.stiffness_n_per_um).speed(0.1).suffix(" N/µm").range(0.0..=1_000.0));
        });
        t.gauge_length = h.gauge_length_mm;
    } else {
        ui.label("No holder assigned.");
    }
}

fn tab_cutting_data(ui: &mut egui::Ui, t: &mut Tool, selected: &mut Option<usize>) {
    ui.horizontal(|ui| {
        if ui.button("+").on_hover_text("Add preset").clicked() {
            let n = t.cutting_presets.len() + 1;
            t.cutting_presets.push(CuttingPreset { name: format!("Preset {n}"), ..CuttingPreset::default() });
            *selected = Some(t.cutting_presets.len() - 1);
        }
        if ui.add_enabled(selected.is_some(), egui::Button::new("Duplicate")).clicked()
            && let Some(i) = *selected
            && let Some(p) = t.cutting_presets.get(i).cloned()
        {
            let mut copy = p;
            copy.name = format!("{} (copy)", copy.name);
            t.cutting_presets.push(copy);
            *selected = Some(t.cutting_presets.len() - 1);
        }
        if ui.add_enabled(selected.is_some(), egui::Button::new("Delete")).clicked()
            && let Some(i) = *selected
            && i < t.cutting_presets.len()
        {
            t.cutting_presets.remove(i);
            *selected = if t.cutting_presets.is_empty() { None } else { Some(i.min(t.cutting_presets.len() - 1)) };
        }
    });

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_min_width(120.0);
            ui.label(RichText::new("Cutting data").strong());
            for (i, p) in t.cutting_presets.iter().enumerate() {
                if ui.selectable_label(*selected == Some(i), &p.name).clicked() {
                    *selected = Some(i);
                }
            }
            if t.cutting_presets.is_empty() {
                ui.label("(none)");
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            let Some(i) = *selected else {
                ui.label("Select or add a preset.");
                return;
            };
            if i >= t.cutting_presets.len() {
                return;
            }
            let surface = t.surface_speed_m_min(t.cutting_presets[i].spindle_rpm);
            let fpt = t.feed_per_tooth(t.cutting_presets[i].cutting_feedrate, t.cutting_presets[i].spindle_rpm);
            let ftp = t.tooth_passing_hz(t.cutting_presets[i].spindle_rpm);
            let p = &mut t.cutting_presets[i];
            labeled_text(ui, "Name", &mut p.name);
            ui.separator();
            ui.label(RichText::new("Speed").strong());
            ui.horizontal(|ui| {
                ui.label("Spindle speed");
                ui.add(egui::DragValue::new(&mut p.spindle_rpm).speed(100).suffix(" rpm").range(1..=120_000));
            });
            ui.label(format!("Surface speed: {:.5} m/min", surface));
            ui.label(format!("Tooth-passing frequency: {:.0} Hz", ftp));
            ui.separator();
            ui.label(RichText::new("Feedrates").strong());
            drag_mm(ui, "Cutting feedrate", &mut p.cutting_feedrate, 1.0, 0.1..=50_000.0);
            ui.label(format!("Feed per tooth: {:.5} mm", fpt));
            drag_mm(ui, "Lead-in feedrate", &mut p.lead_in_feedrate, 1.0, 0.1..=50_000.0);
            drag_mm(ui, "Lead-out feedrate", &mut p.lead_out_feedrate, 1.0, 0.1..=50_000.0);
            drag_mm(ui, "Transition feedrate", &mut p.transition_feedrate, 1.0, 0.1..=50_000.0);
            drag_mm(ui, "Ramp feedrate", &mut p.ramp_feedrate, 1.0, 0.1..=50_000.0);
            ui.horizontal(|ui| {
                ui.label("Ramp angle");
                ui.add(egui::DragValue::new(&mut p.ramp_angle_deg).speed(0.1).suffix(" °").range(0.1..=90.0));
            });
            ui.separator();
            ui.label(RichText::new("Vertical feedrates").strong());
            drag_mm(ui, "Plunge feedrate", &mut p.plunge_feedrate, 1.0, 0.1..=50_000.0);
            if p.spindle_rpm > 0 {
                ui.label(format!("Plunge feed per revolution: {:.5} mm", p.plunge_feedrate / f64::from(p.spindle_rpm)));
            }
            ui.separator();
            ui.label(RichText::new("Passes and linking").strong());
            ui.checkbox(&mut p.use_stepdown, "Use stepdown");
            if p.use_stepdown {
                drag_mm(ui, "Stepdown", &mut p.stepdown_mm, 0.01, 0.01..=100.0);
            }
            ui.checkbox(&mut p.use_stepover, "Use stepover");
            if p.use_stepover {
                drag_mm(ui, "Stepover", &mut p.stepover_mm, 0.01, 0.01..=100.0);
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Coolant");
                egui::ComboBox::from_id_salt("edit_preset_coolant").selected_text(p.coolant.label()).show_ui(ui, |ui| {
                    for c in Coolant::ALL {
                        if ui.selectable_label(p.coolant == *c, c.label()).clicked() {
                            p.coolant = *c;
                        }
                    }
                });
            });
        });
    });
}

fn tab_physics(ui: &mut egui::Ui, t: &mut Tool) {
    ui.label(RichText::new("Compliance chain").strong());
    drag_mm(ui, "Stick-out", &mut t.length_below_holder, 0.1, 0.0..=1000.0);
    drag_mm(ui, "Core diameter", &mut t.shaft_diameter, 0.01, 0.0..=500.0);
    ui.label(format!("Young's modulus: {:.0} GPa", t.youngs_gpa()));
    let k_tool = t.stiffness_n_per_um();
    ui.label(format!("Estimated tool stiffness: {:.2} N/µm", k_tool));
    if let Some(h) = t.holder.as_mut() {
        ui.horizontal(|ui| {
            ui.label("Holder stiffness");
            ui.add(egui::DragValue::new(&mut h.stiffness_n_per_um).speed(0.1).suffix(" N/µm").range(0.0..=1_000.0));
        });
    } else {
        ui.label("No holder assigned.");
    }
    if t.holder.is_some() {
        ui.label(format!("Assembly stiffness: {:.2} N/µm", t.assembly_stiffness_n_per_um()));
    }
    let defl_mm = if k_tool > 1e-9 { 100.0 / (k_tool * 1_000.0) } else { 0.0 };
    ui.label(format!("Deflection at 100 N: {:.3} mm", defl_mm));
    let e = t.youngs_gpa() * 1_000.0;
    let i = std::f64::consts::PI / 64.0 * t.core_diameter().max(0.1).powi(4);
    let k_allow_n_per_mm = 100.0 / 0.05;
    let max_stick = ((3.0 * e * i) / k_allow_n_per_mm).cbrt();
    ui.label(format!("Max stick-out for 0.05 mm at 100 N: {:.1} mm", max_stick));
    if k_tool < 2.0 {
        ui.label("This assembly is the likely weak link on thin walls. Shorten stick-out or finish those walls last.");
    }
    ui.separator();
    ui.label(RichText::new("Cutting").strong());
    if let Some(p) = t.cutting_presets.first() {
        ui.label(format!("Tooth-passing frequency: {:.0} Hz", t.tooth_passing_hz(p.spindle_rpm)));
    } else {
        ui.label("Add a cutting-data preset to see tooth-passing frequency.");
    }
    ui.separator();
    ui.label("Finish thin walls in alternating bands from the top down. Lower feed last.");
    ui.label("Re-clamp at lower torque after roughing.");
}

fn tab_post(ui: &mut egui::Ui, t: &mut Tool) {
    ui.horizontal(|ui| {
        ui.label("Number");
        ui.add(egui::DragValue::new(&mut t.number).speed(1).range(1..=999));
    });
    ui.horizontal(|ui| {
        ui.label("Length offset");
        ui.add(egui::DragValue::new(&mut t.length_offset).speed(1).range(0..=999));
    });
    ui.horizontal(|ui| {
        ui.label("Diameter offset");
        ui.add(egui::DragValue::new(&mut t.diameter_offset).speed(1).range(0..=999));
    });
    ui.horizontal(|ui| {
        ui.label("Turret");
        ui.add(egui::DragValue::new(&mut t.turret).speed(1).range(0..=99));
    });
    ui.label("Comment");
    ui.text_edit_multiline(&mut t.comment);
    ui.checkbox(&mut t.manual_tool_change, "Manual tool change");
    ui.checkbox(&mut t.live_tool, "Live tool");
    ui.checkbox(&mut t.break_control, "Break control");
}

fn tool_preview(ui: &mut egui::Ui, t: &Tool) {
    let (rect, _resp) = ui.allocate_exact_size(egui::vec2(200.0, 220.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(28, 36, 48));
    // Simple 2D silhouette: holder block + shaft + flute tip.
    let cx = rect.center().x;
    let tip_y = rect.bottom() - 24.0;
    let flute_h = (t.flute_length.max(1.0) / t.length.max(1.0) * 70.0).clamp(12.0, 70.0) as f32;
    let below_h = (t.length_below_holder.max(1.0) / t.length.max(1.0) * 90.0).clamp(20.0, 100.0) as f32;
    let flute_w = (t.diameter / t.shaft_diameter.max(t.diameter).max(0.1) * 10.0).clamp(2.0, 14.0) as f32;
    let shaft_w = flute_w.max(6.0) + 4.0;
    let holder_w = shaft_w + 28.0;

    let holder_top = tip_y - below_h - 50.0;
    let holder_bot = tip_y - below_h;
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(cx - holder_w * 0.5, holder_top), egui::pos2(cx + holder_w * 0.5, holder_bot)),
        1.0,
        egui::Color32::from_rgb(120, 150, 180),
    );
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(cx - shaft_w * 0.5, holder_bot), egui::pos2(cx + shaft_w * 0.5, tip_y - flute_h)),
        0.0,
        egui::Color32::from_rgb(160, 165, 170),
    );
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(cx - flute_w * 0.5, tip_y - flute_h), egui::pos2(cx + flute_w * 0.5, tip_y)),
        0.0,
        egui::Color32::from_rgb(210, 180, 60),
    );
    painter.text(
        egui::pos2(rect.right() - 8.0, rect.bottom() - 8.0),
        egui::Align2::RIGHT_BOTTOM,
        "5 mm",
        egui::FontId::proportional(10.0),
        egui::Color32::from_rgb(180, 190, 200),
    );
    painter.text(
        egui::pos2(rect.left() + 6.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        t.kind.label(),
        egui::FontId::proportional(11.0),
        egui::Color32::from_rgb(200, 210, 220),
    );
}

fn labeled_text(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(value);
    });
}

fn drag_mm(ui: &mut egui::Ui, label: &str, value: &mut f64, speed: f64, range: std::ops::RangeInclusive<f64>) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).speed(speed).range(range).suffix(" mm"));
    });
}

fn save_edit(app: &mut SolveApp, st: &EditToolState) -> bool {
    let tools = ui_tools(app);
    if let Some(i) = st.tool_index {
        if let Some(old) = tools.get(i)
            && old.number != st.draft.number
        {
            let _ = persist_remove(app, old.number);
        }
        app.manufacture_workspace.selected_tool = Some(i);
    } else {
        app.manufacture_workspace.selected_tool = Some(tools.len());
    }
    persist_tool(app, &st.draft)
}

fn save_tools(app: &mut SolveApp, _setup_name: &str, tools: Vec<Tool>) -> bool {
    let existing: Vec<u32> = app.session.doc.cam.tools.iter().map(|t| t.number).collect();
    let keep: Vec<u32> = tools.iter().map(|t| t.number).collect();
    for n in existing {
        if !keep.contains(&n) && !persist_remove(app, n) {
            return false;
        }
    }
    for t in &tools {
        if !persist_tool(app, t) {
            return false;
        }
    }
    true
}

fn persist_tool(app: &mut SolveApp, t: &Tool) -> bool {
    let kind = match t.kind {
        ToolKind::BallEndmill | ToolKind::BullnoseEndmill => "ball_endmill",
        ToolKind::Drill | ToolKind::Tap | ToolKind::Probe => "drill",
        _ => "flat_endmill",
    };
    let preset = t.cutting_presets.first();
    let mut params = json!({
        "number": t.number,
        "kind": kind,
        "diameter": t.diameter,
        "flute_length": t.flute_length,
        "description": t.description,
        "flutes": t.flutes,
        "stick_out": t.length_below_holder,
        "shaft_diameter": t.shaft_diameter,
        "material": t.material.param(),
    });
    if t.gauge_length > 0.0 {
        params["holder_gauge"] = json!(t.gauge_length);
    }
    if let Some(h) = t.holder.as_ref()
        && h.stiffness_n_per_um > 0.0
    {
        params["holder_stiffness"] = json!(h.stiffness_n_per_um);
        params["holder_gauge"] = json!(h.gauge_length_mm);
    }
    if let Some(p) = preset {
        params["spindle_rpm"] = json!(p.spindle_rpm);
        params["feed"] = json!(p.cutting_feedrate);
        params["plunge_feed"] = json!(p.plunge_feedrate);
        if p.use_stepdown {
            params["stepdown"] = json!(p.stepdown_mm);
        }
        if p.use_stepover {
            params["stepover"] = json!(p.stepover_mm);
        }
    }
    match app.run("cam.tool.define", params) {
        Ok(_) => true,
        Err(e) => {
            app.set_status(e, true);
            false
        }
    }
}

fn persist_remove(app: &mut SolveApp, number: u32) -> bool {
    match app.run("cam.tool.remove", json!({"number": number})) {
        Ok(_) => true,
        Err(e) => {
            app.set_status(e, true);
            false
        }
    }
}

fn ui_tools(app: &SolveApp) -> Vec<Tool> {
    app.session
        .doc
        .cam
        .tools
        .iter()
        .map(|t| {
            let kind = match t.kind.label() {
                "ball end mill" => ToolKind::BallEndmill,
                "drill" => ToolKind::Drill,
                _ => ToolKind::FlatEndmill,
            };
            let mut draft = Tool {
                number: t.number,
                kind,
                diameter: t.diameter,
                flute_length: t.flute_length,
                description: t.description.clone(),
                flutes: t.flutes.min(255) as u8,
                length_offset: t.number,
                diameter_offset: t.number,
                length_below_holder: t.stick_out.unwrap_or(30.0),
                shaft_diameter: t.shaft_diameter.unwrap_or(t.diameter),
                material: t.material.as_deref().map(ToolMaterial::from_param).unwrap_or(ToolMaterial::Carbide),
                gauge_length: t.holder_gauge.unwrap_or(80.0),
                ..Tool::default()
            };
            if let Some(k) = t.holder_stiffness.filter(|k| *k > 0.0) {
                draft.holder = Some(ToolHolder {
                    description: String::new(),
                    product_id: String::new(),
                    product_link: String::new(),
                    vendor: String::new(),
                    gauge_length_mm: t.holder_gauge.unwrap_or(draft.gauge_length),
                    stiffness_n_per_um: k,
                });
            }
            if t.spindle_rpm.is_some() || t.feed.is_some() {
                let mut p = CuttingPreset::default();
                if let Some(rpm) = t.spindle_rpm {
                    p.spindle_rpm = rpm.max(0.0) as u32;
                }
                if let Some(feed) = t.feed {
                    p.cutting_feedrate = feed;
                }
                if let Some(plunge) = t.plunge_feed {
                    p.plunge_feedrate = plunge;
                }
                if let Some(sd) = t.stepdown {
                    p.use_stepdown = true;
                    p.stepdown_mm = sd;
                }
                if let Some(so) = t.stepover {
                    p.use_stepover = true;
                    p.stepover_mm = so;
                }
                draft.cutting_presets.push(p);
            }
            draft
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carbide_stick_out_stiffness_matches_stock_integrity_example() {
        let t = Tool {
            diameter: 10.0,
            shaft_diameter: 8.0,
            length_below_holder: 30.0,
            material: ToolMaterial::Carbide,
            ..Tool::default()
        };
        let k = t.stiffness_n_per_um();
        assert!((k - 13.4).abs() < 0.1, "{k}");
        assert!((t.tooth_passing_hz(12_000) - 800.0).abs() < 1e-9);
    }
}
