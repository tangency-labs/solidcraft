//! Local Weld form; the dialog adapter saves its settings and supported symbol through commands.
//! The hidden public module is solely the native preview's entry point.

use egui::{Rect, Ui};
use solidcraft_theme::Tokens;

#[path = "weld_panel_sections.rs"]
mod sections;
#[path = "weld_panel/symbol.rs"]
pub(crate) mod symbol;
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
    Butt, "butt", "weld_type_groove", Line, true, false, false, 0;
    Edge, "edge", "weld_type_edge", Line, false, true, false, 0;
    Plug, "plug", "weld_type_plug_slot", Hole, false, true, false, 1;
    Spot, "spot", "weld_type_spot", Point, true, false, false, 2;
    Seam, "seam", "weld_type_resistance_seam", Line, false, true, false, 0;
    Stud, "stud", "weld_type_stud", Ring, false, true, false, 3;
    Surfacing, "surfacing", "weld_type_overlay", Area, false, true, false, 4;
    Tack, "tack", "weld_flag_tack", Line, true, true, false, 0;
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
    pub z2: String,
    pub h: String,
    pub s: String,
    pub d: String,
    pub n: String,
    pub l: String,
    pub e: String,
    pub t: String,
    pub w: String,
}
impl Default for SizeValues {
    fn default() -> Self {
        Self {
            a: "3.0".into(),
            z: "4.2".into(),
            z1: String::new(),
            z2: String::new(),
            h: "1.0".into(),
            s: String::new(),
            d: "5.0".into(),
            n: "4".into(),
            l: String::new(),
            e: String::new(),
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
    pub symbol_standard: String,
    pub groove: usize,
    pub partial_penetration: bool,
    pub root: usize,
    pub root_gap: String,
    pub add_fillet: bool,
    pub focus_dimension: Option<&'static str>,
    pub picks: Picks,
    pub size: SizeValues,
    pub size_by_leg: bool,
    pub side: usize,
    pub face: Face,
    pub blend_toes: bool,
    pub sheet_picked: bool,
    pub tangent_chain: bool,
    pub span_count: String,
    pub span_pitch: String,
    pub span_length: String,
    pub spans: bool,
    pub both_sides: bool,
    pub staggered: bool,
    pub ends: usize,
    pub sharp_mitre: bool,
    pub merge_crossing: bool,
    pub different_halves: bool,
    pub field_weld: bool,
    pub all_around: bool,
    pub process: String,
    pub process_search: String,
    pub spot_look: usize,
    pub passes_shown: bool,
    pub penetration_shown: bool,
    pub advanced_open: bool,
    pub density: String,
    pub overlap: String,
    pub reference_plane: usize,
    pub notices: Vec<WeldNotice>,
    pub type_picker_open: bool,
    pub tack_mode: usize,
}

impl Default for WeldPanelState {
    fn default() -> Self {
        Self {
            number: "W1".into(),
            weld_type: WeldType::Fillet,
            symbol_standard: "iso2553_2019_a".into(),
            groove: 1,
            partial_penetration: false,
            root: 0,
            root_gap: String::new(),
            add_fillet: false,
            focus_dimension: None,
            picks: Picks::default(),
            size: SizeValues::default(),
            size_by_leg: false,
            side: 0,
            face: Face::Convex,
            blend_toes: false,
            sheet_picked: false,
            tangent_chain: false,
            span_count: "4".into(),
            span_pitch: "40".into(),
            span_length: "10".into(),
            spans: false,
            both_sides: false,
            staggered: false,
            ends: 0,
            sharp_mitre: false,
            merge_crossing: true,
            different_halves: false,
            field_weld: false,
            all_around: false,
            process: "135".into(),
            process_search: String::new(),
            spot_look: 0,
            passes_shown: false,
            penetration_shown: false,
            advanced_open: false,
            density: String::new(),
            overlap: String::new(),
            reference_plane: 0,
            notices: Vec::new(),
            type_picker_open: false,
            tack_mode: 0,
        }
    }
}

impl WeldPanelState {
    pub fn where_kind(&self) -> WhereKind {
        if self.weld_type == WeldType::Tack && self.tack_mode == 2 { WhereKind::Point } else { self.weld_type.info().kind }
    }
    pub fn pick_type(&mut self, weld_type: WeldType) {
        self.weld_type = weld_type;
        self.process_search.clear();
        if weld_type == WeldType::Tack {
            if self.size.l.trim().is_empty() {
                self.size.l = "5".into();
            }
            if self.size.e.trim().is_empty() {
                self.size.e = "40".into();
            }
        }
        self.face = match weld_type {
            WeldType::Butt | WeldType::Plug => Face::Flat,
            _ => Face::Convex,
        };
        self.process = match weld_type {
            WeldType::Edge => "141",
            WeldType::Spot => "21",
            WeldType::Seam => "22",
            WeldType::Stud => "78",
            _ => "135",
        }
        .into();
    }

    /// Board specimens; real callers supply their own references and notices.
    pub fn specimen(weld_type: WeldType, advanced: bool) -> Self {
        let labels = |ids: &[&str]| ids.iter().map(|id| tr(id).to_owned()).collect();
        let mut state = Self {
            weld_type,
            advanced_open: advanced,
            sheet_picked: advanced,
            picks: Picks {
                line: labels(&["ui.weld.pick.edge1", "ui.weld.pick.edge2"]),
                groove: labels(&["ui.weld.pick.plate_a", "ui.weld.pick.plate_b"]),
                point: labels(&["ui.weld.pick.point1", "ui.weld.pick.point2", "ui.weld.pick.point3"]),
                hole: labels(&["ui.weld.pick.slot1"]),
                ring: labels(&["ui.weld.pick.foot"]),
                area: labels(&["ui.weld.pick.face1", "ui.weld.pick.face2"]),
            },
            ..Self::default()
        };
        state.pick_type(weld_type);
        match weld_type {
            WeldType::Fillet => state.notices = WeldNotice::fillet_samples(),
            WeldType::Butt => {
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
    pub header: Option<Rect>,
    pub body: Option<Rect>,
    pub preview: Option<Rect>,
    pub footer: Option<Rect>,
    pub define: Option<Rect>,
    pub cancel: Option<Rect>,
    pub ok: Option<Rect>,
    pub apply: Option<Rect>,
}

/// Scrollable content only; the host supplies the shared tool-panel chrome.
pub fn show_body(ui: &mut Ui, state: &mut WeldPanelState, tokens: &Tokens) -> WeldPanelEvent {
    sections::show_body_measured(ui, state, tokens).0
}

/// Bounded tool card, 344 points wide in the standard app metrics.
pub fn show(ui: &mut Ui, state: &mut WeldPanelState, tokens: &Tokens) -> WeldPanelEvent {
    sections::show_measured(ui, state, tokens).0
}

/// Contents of a Panel already placed by the live dialog host.
pub(crate) fn show_contents(
    ui: &mut Ui,
    state: &mut WeldPanelState,
    tokens: &Tokens,
    height: f32,
    extra_body: impl FnOnce(&mut Ui),
) -> WeldPanelEvent {
    sections::show_contents_measured(ui, state, tokens, height, extra_body).0
}
