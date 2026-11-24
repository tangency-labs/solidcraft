//! Board-driven, local-only weld editor. No weld command or model evaluation is wired yet.
//! The hidden public module is solely the native preview's entry point.

use egui::{Rect, Ui};
use solidcraft_theme::Tokens;

#[path = "weld_panel_sections.rs"]
mod sections;
#[path = "weld_panel/symbol.rs"]
mod symbol;
#[cfg(test)]
#[path = "weld_panel_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhereKind {
    Line,
    Point,
    Hole,
    Ring,
    Area,
}

#[derive(Clone, Copy, Debug)]
pub struct TypeInfo {
    pub weld_type: WeldType,
    pub label: &'static str,
    pub icon: &'static str,
    pub kind: WhereKind,
    pub visible: bool,
    pub soon: bool,
    pub look_tbd: bool,
    pub family: usize,
}

// One source of type metadata; the four quick choices and the family picker share it.
macro_rules! types {
    ($( $variant:ident, $id:literal, $icon:literal, $kind:ident, $visible:literal, $soon:literal, $tbd:literal, $family:literal; )*) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub enum WeldType { #[default] Fillet, $($variant,)* }
        pub const TYPES: &[TypeInfo] = &[
            TypeInfo { weld_type: WeldType::Fillet, label: "ui.weld.type.fillet", icon: "weld_type_fillet", kind: WhereKind::Line, visible: true, soon: false, look_tbd: false, family: 0 },
            $(TypeInfo { weld_type: WeldType::$variant, label: concat!("ui.weld.type.", $id), icon: $icon, kind: WhereKind::$kind, visible: $visible, soon: $soon, look_tbd: $tbd, family: $family },)*
        ];
    }
}
types! {
    Groove, "groove", "weld_type_groove", Line, true, false, false, 0;
    GrooveFillet, "groove_fillet", "weld_type_groove_fillet", Line, false, true, false, 0;
    RootGap, "root_gap", "weld_type_root_gap", Line, false, true, false, 0;
    Edge, "edge", "weld_type_edge", Line, false, true, false, 0;
    FlangedButt, "flanged_butt", "weld_type_flanged_butt", Line, false, true, false, 0;
    FlangedCorner, "flanged_corner", "weld_type_flanged_corner", Line, false, true, false, 0;
    BeadLine, "bead_line", "weld_type_bead_line", Line, false, true, false, 0;
    SeamOnly, "seam_only", "weld_type_seam_only", Line, false, true, false, 0;
    Rip, "rip", "weld_type_sheet_rip", Line, false, true, false, 0;
    Spot, "spot", "weld_type_spot", Point, true, false, false, 1;
    Projection, "projection", "weld_type_projection", Point, false, true, false, 1;
    FusionSpot, "fusion_spot", "weld_type_fusion_spot", Point, false, true, false, 1;
    Stake, "stake", "weld_type_stake", Point, false, true, false, 1;
    Plug, "plug", "weld_type_plug_slot", Hole, true, true, false, 1;
    Stud, "stud", "weld_type_stud", Ring, false, true, false, 1;
    ResistanceSeam, "res_seam", "weld_type_resistance_seam", Line, false, true, false, 2;
    FusionSeam, "fusion_seam", "weld_type_fusion_seam", Line, false, true, false, 2;
    Overlay, "overlay", "weld_type_overlay", Area, false, true, false, 3;
    Friction, "friction", "weld_type_friction", Ring, false, true, false, 4;
    FrictionStir, "fsw", "weld_type_friction_stir", Line, false, true, false, 4;
    Ultrasonic, "ultrasonic", "weld_type_ultrasonic", Point, false, true, true, 4;
    Explosion, "explosion", "weld_type_explosion", Area, false, true, true, 4;
    Diffusion, "diffusion", "weld_type_diffusion", Area, false, true, true, 4;
    Magnetic, "magnetic", "weld_type_magnetic_pulse", Ring, false, true, true, 4;
    Cold, "cold", "weld_type_cold_pressure", Point, false, true, true, 4;
}

impl WeldType {
    pub fn info(self) -> &'static TypeInfo {
        // Exhaustive table above; use a safe fallback if a future variant omits its row.
        TYPES.iter().find(|t| t.weld_type == self).unwrap_or(&TYPES[0])
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Face {
    Flat,
    #[default]
    Convex,
    Concave,
}

#[derive(Clone, Debug, Default)]
pub struct Picks {
    pub line: Vec<String>,
    pub groove: Vec<String>,
    pub point: Vec<String>,
    pub hole: Vec<String>,
    pub ring: Vec<String>,
    pub area: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct SizeValues {
    pub a: String,
    pub z: String,
    pub z1: String,
    pub h: String,
    pub s: String,
    pub d: String,
    pub fill: String,
    pub collar: String,
    pub t: String,
    pub w: String,
}
impl Default for SizeValues {
    fn default() -> Self {
        Self {
            a: "3.0".into(),
            z: "4.2".into(),
            z1: "3.0".into(),
            h: "1.0".into(),
            s: String::new(),
            d: "5.0".into(),
            fill: "20".into(),
            collar: "3.0".into(),
            t: "2.0".into(),
            w: "6.0".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Warn,
    Clash,
    Refusal,
    Lost,
    Info,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoticeAction {
    UseFit(String),
    PickAgain,
    Stretches(Vec<String>),
}

#[derive(Clone, Debug)]
pub struct WeldNotice {
    pub kind: NoticeKind,
    /// A catalog id, never an untranslated sentence.
    pub message: &'static str,
    pub action: Option<NoticeAction>,
}

impl WeldNotice {
    pub fn fillet_samples() -> Vec<Self> {
        vec![
            Self { kind: NoticeKind::Warn, message: "ui.weld.notice.tight", action: None },
            Self { kind: NoticeKind::Refusal, message: "ui.weld.notice.fit", action: Some(NoticeAction::UseFit("6.2".into())) },
        ]
    }
    pub fn all_samples() -> Vec<Self> {
        let mut items = Self::fillet_samples();
        items.extend([
            Self { kind: NoticeKind::Clash, message: "ui.weld.notice.clash", action: None },
            Self {
                kind: NoticeKind::Refusal,
                message: "ui.weld.notice.section",
                action: Some(NoticeAction::Stretches(vec!["ui.weld.stretch.first".into(), "ui.weld.stretch.second".into()])),
            },
            Self { kind: NoticeKind::Warn, message: "ui.weld.notice.iso", action: None },
            Self { kind: NoticeKind::Lost, message: "ui.weld.notice.lost", action: Some(NoticeAction::PickAgain) },
            Self { kind: NoticeKind::Info, message: "ui.weld.notice.depth", action: None },
        ]);
        items
    }
}

/// All edits are local data; the future command adapter consumes the returned event.
#[derive(Clone, Debug)]
pub struct WeldPanelState {
    pub number: String,
    pub weld_type: WeldType,
    pub picks: Picks,
    pub size: SizeValues,
    pub size_by_leg: bool,
    pub root_side: usize,
    pub face: Face,
    pub blend_toes: bool,
    pub sheet_picked: bool,
    pub tangent_chain: bool,
    pub flip_side: bool,
    pub pattern_count: String,
    pub pattern_pitch: String,
    pub edge_distance: String,
    pub spans: bool,
    pub both_sides: bool,
    pub staggered: bool,
    pub ends: usize,
    pub sharp_mitre: bool,
    pub merge_crossing: bool,
    pub different_halves: bool,
    pub tack: bool,
    pub field_weld: bool,
    pub all_around: bool,
    pub process: usize,
    pub texture: usize,
    pub spot_look: usize,
    pub passes_shown: bool,
    pub penetration_shown: bool,
    pub advanced_open: bool,
    pub density: String,
    pub overlap: String,
    pub reference_plane: usize,
    pub notices: Vec<WeldNotice>,
    pub type_picker_open: bool,
}

impl Default for WeldPanelState {
    fn default() -> Self {
        Self {
            number: "W1".into(),
            weld_type: WeldType::Fillet,
            picks: Picks::default(),
            size: SizeValues::default(),
            size_by_leg: false,
            root_side: 0,
            face: Face::Convex,
            blend_toes: false,
            sheet_picked: false,
            tangent_chain: false,
            flip_side: false,
            pattern_count: "4".into(),
            pattern_pitch: "40".into(),
            edge_distance: "12".into(),
            spans: false,
            both_sides: false,
            staggered: false,
            ends: 0,
            sharp_mitre: false,
            merge_crossing: true,
            different_halves: false,
            tack: false,
            field_weld: false,
            all_around: false,
            process: 0,
            texture: 0,
            spot_look: 0,
            passes_shown: false,
            penetration_shown: false,
            advanced_open: false,
            density: "7850".into(),
            overlap: "1.5".into(),
            reference_plane: 0,
            notices: Vec::new(),
            type_picker_open: false,
        }
    }
}

impl WeldPanelState {
    /// Board specimens; real callers supply their own references and notices.
    pub fn specimen(weld_type: WeldType, advanced: bool) -> Self {
        let labels = |ids: &[&str]| ids.iter().map(|id| tr(id).to_owned()).collect();
        let mut state = Self {
            weld_type,
            advanced_open: advanced,
            sheet_picked: advanced,
            tack: advanced,
            picks: Picks {
                line: labels(&["ui.weld.pick.edge1", "ui.weld.pick.edge2"]),
                groove: labels(&["ui.weld.pick.plate_a", "ui.weld.pick.plate_b"]),
                point: labels(&["ui.weld.pick.point1", "ui.weld.pick.point2", "ui.weld.pick.point3"]),
                hole: labels(&["ui.weld.pick.slot1"]),
                ring: labels(&["ui.weld.pick.slot1", "ui.weld.pick.foot"]),
                area: labels(&["ui.weld.pick.face1", "ui.weld.pick.face2"]),
            },
            ..Self::default()
        };
        match weld_type {
            WeldType::Fillet => state.notices = WeldNotice::fillet_samples(),
            WeldType::Groove => {
                state.face = Face::Flat;
                state.process = 1;
                state.texture = 1;
                state.notices = vec![
                    WeldNotice {
                        kind: NoticeKind::Refusal,
                        message: "ui.weld.notice.section",
                        action: Some(NoticeAction::Stretches(vec!["ui.weld.stretch.first".into(), "ui.weld.stretch.second".into()])),
                    },
                    WeldNotice { kind: NoticeKind::Info, message: "ui.weld.notice.depth", action: None },
                ];
            }
            WeldType::Spot => state.notices.push(WeldNotice { kind: NoticeKind::Refusal, message: "ui.weld.notice.sheets", action: None }),
            WeldType::Plug => {
                state.face = Face::Flat;
            }
            WeldType::Stud => {
                state.size.fill = "4".into();
                state.process = 6;
            }
            WeldType::Overlay => {
                state.process = 3;
                state.texture = 3;
            }
            WeldType::FrictionStir => {
                state.process = 5;
                state.texture = 5;
            }
            _ => {}
        }
        state
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum WeldPanelEvent {
    #[default]
    None,
    Done,
    Cancel,
    DefineSymbol,
    NewBead,
    TypePicked(WeldType),
    Pick(WhereKind),
    PicksChanged,
    UseFit(usize),
    PickAgain(usize),
    InspectNotice(usize),
    WeldStretch(usize, usize),
}

pub(super) fn tr(id: &str) -> &str {
    solidcraft_ui_text::tr(id)
}

#[cfg(test)]
pub(super) struct ChoiceMeasurements {
    pub button: Rect,
    pub icon: Rect,
    pub label: Rect,
    pub lines: usize,
}

#[derive(Default)]
pub(super) struct Measurements {
    pub card: Option<Rect>,
    pub sections: Vec<(&'static str, Rect)>,
    pub content: Option<Rect>,
    pub figure: Option<Rect>,
    pub fields: Vec<Rect>,
    pub field_column: Option<Rect>,
    pub widgets: Vec<Rect>,
    pub notice_actions: Vec<(usize, Rect)>,
    #[cfg(test)]
    pub type_choices: Vec<ChoiceMeasurements>,
    pub more_button: Option<Rect>,
    #[cfg(test)]
    pub done_check: Option<Rect>,
    #[cfg(test)]
    pub strip_hint: Option<Rect>,
    #[cfg(test)]
    pub strip_hint_text: Option<Rect>,
    #[cfg(test)]
    pub strip_hint_lines: usize,
}

/// Content-height floating card, 344 points wide in the standard app metrics.
pub fn show(ui: &mut Ui, state: &mut WeldPanelState, tokens: &Tokens) -> WeldPanelEvent {
    sections::show_measured(ui, state, tokens).0
}
