use egui::{Stroke, vec2};
use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_engine::doc::drawing::{Drawing, view::Beside};
use solidcraft_engine::doc::ids::encode;
use solidcraft_engine::geom::Vec3Geom;
use solidcraft_engine::geom::drawing_view::ProjectedSide;
#[cfg(not(target_arch = "wasm32"))]
use solidcraft_engine::geom::paper::Scene;
use solidcraft_engine::render::VisualStyle;
use solidcraft_theme::Tokens;

use crate::{SolveApp, drawing_sheet_page::Marks, ui_kit};

const ORIENTATIONS: [(&str, &str); 7] = [
    ("front", "ui.front"),
    ("top", "ui.top"),
    ("right", "ui.right"),
    ("left", "ui.drawing_panel.left"),
    ("bottom", "ui.bottom"),
    ("back", "ui.back"),
    ("iso", "ui.drawing_panel.iso"),
];
const SCALES: [&str; 5] = ["1:4", "1:2", "1:1", "2:1", "4:1"];
const STYLES: [VisualStyle; 2] = [VisualStyle::Visible, VisualStyle::VisibleHidden];

pub(super) struct Placement {
    owner: (u64, u64, u64),
    orientation: usize,
    scale: usize,
    style: usize,
    parent: Option<u64>,
    key: Option<(u64, Value)>,
    preview: Option<Marks>,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<std::sync::mpsc::Receiver<Option<Marks>>>,
}

impl Placement {
    pub(super) fn new(app: &SolveApp) -> Option<Self> {
        Some(Self {
            owner: (app.session.uid, app.session.drawing("drawing.view.base").ok()?.id, super::active_sheet(app)?),
            orientation: 0,
            scale: 2,
            style: 0,
            parent: None,
            key: None,
            preview: None,
            #[cfg(not(target_arch = "wasm32"))]
            worker: None,
        })
    }

    pub(super) fn from_parent(app: &SolveApp, parent: u64) -> Option<Self> {
        let mut placement = Self::new(app)?;
        let drawing = app.session.drawing("drawing.view.projected").ok()?;
        if drawing.view(parent)?.sheet != placement.owner.2 {
            return None;
        }
        drawing.can_project_from(parent).ok()?;
        placement.parent = Some(parent);
        Some(placement)
    }

    fn orientation(&self) -> &str {
        ORIENTATIONS.get(self.orientation).unwrap_or(&ORIENTATIONS[0]).0
    }

    fn scale(&self) -> &str {
        SCALES.get(self.scale).copied().unwrap_or("1:1")
    }

    fn style(&self) -> VisualStyle {
        STYLES.get(self.style).copied().unwrap_or_default()
    }

    fn target(&self, drawing: &Drawing, cursor: Vec2) -> Option<(Vec2, Option<Beside>, Value)> {
        let Some(parent) = self.parent else {
            return Some((cursor, None, json!({"view":self.orientation(),"scale":self.scale(),"visual_style":self.style()})));
        };
        let centre = drawing.view_centre(parent).ok()?;
        let beside = Beside::of_offset(cursor - centre)?;
        drawing.can_project_from(parent).ok()?;
        let resolved = drawing.resolve_view(parent).ok()?;
        let basis = beside.basis(&resolved.basis, drawing.projection).ok()?;
        let at = match beside {
            Beside::Side(ProjectedSide::Left | ProjectedSide::Right) => Vec2::new(cursor.x, centre.y),
            Beside::Side(ProjectedSide::Above | ProjectedSide::Below) => Vec2::new(centre.x, cursor.y),
            Beside::Diagonal(_) => cursor,
        };
        let direction = basis.y.cross(basis.x);
        let request = json!({
            "view":{"direction":[direction.x,direction.y,direction.z],"up":[basis.y.x,basis.y.y,basis.y.z]},
            "scale":resolved.scale.value.to_string(),
            "visual_style":resolved.visual_style.value
        });
        Some((at, Some(beside), request))
    }

    pub(super) fn fields(&mut self, ui: &mut egui::Ui, source: &str) {
        let tr = solidcraft_ui_text::tr;
        ui_kit::section_heading(ui, tr("ui.drawing_panel.orientation"));
        ui_kit::text(
            ui,
            tr(if self.parent.is_some() { "ui.drawing_placement.projected" } else { "ui.drawing_placement.base" }),
            ui_kit::Text::Note("drawing_placement"),
        );
        if self.parent.is_none() {
            ui_kit::text(ui, tr("ui.drawing_panel.face_pick"), ui_kit::Text::Note("base_view_face_pick"));
        }
        ui.add_enabled_ui(self.parent.is_none(), |ui| {
            ui.horizontal_wrapped(|ui| {
                for (index, (orientation, label)) in ORIENTATIONS.iter().enumerate() {
                    let style = if index == self.orientation { ui_kit::Button::SmallSelected } else { ui_kit::Button::Small };
                    let response = ui_kit::button(ui, tr(label), style);
                    crate::scenario::publish_target(&format!("drawing:placement:orientation:{orientation}"), response.rect);
                    if response.clicked() {
                        self.orientation = index;
                    }
                }
            });
            ui.add_enabled_ui(false, |ui| {
                ui_kit::choice_row(ui, tr("ui.drawing_panel.saved_view"), "base_view_saved", &mut 0, &[tr("ui.drawing_panel.none")]);
            });
            ui_kit::section_heading(ui, tr("ui.drawing_panel.view"));
            ui.add_enabled_ui(false, |ui| {
                ui_kit::choice_row(ui, tr("ui.drawing_panel.model"), "base_view_model", &mut 0, &[source]);
            });
            ui_kit::choice_row(ui, tr("ui.scale"), "base_view_scale", &mut self.scale, &SCALES);
            let styles = ["ui.drawing_panel.visible", "ui.drawing_panel.hidden"].map(tr);
            ui_kit::choice_row(ui, tr("ui.drawing_panel.visual_style"), "base_view_style", &mut self.style, &styles);
        });
        if self.parent.is_none() {
            super::panels::pending(ui, tr("ui.drawing_panel.standard_views"));
        }
    }

    fn update_preview(&mut self, app: &SolveApp, ctx: &egui::Context, mut params: Value) {
        params["position_mm"] = json!([0.0, 0.0]);
        params["scene"] = json!(true);
        params["chord_mm"] = json!(0.1);
        let key = (app.session.revision, params);
        let changed = self.key.as_ref() != Some(&key);
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(receiver) = &self.worker {
                match receiver.try_recv() {
                    Ok(preview) if !changed => self.preview = preview,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        ctx.request_repaint_after(std::time::Duration::from_millis(16));
                        if changed {
                            self.preview = None;
                        }
                        return;
                    }
                    _ => {}
                }
                self.worker = None;
            }
            if changed {
                let session = app.session.scratch();
                let request = key.1.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                let repaint = ctx.clone();
                if std::thread::Builder::new()
                    .name("drawing placement preview".into())
                    .spawn(move || {
                        let preview = snapshot_marks(session, &request);
                        let _ = tx.send(preview);
                        repaint.request_repaint();
                    })
                    .is_ok()
                {
                    self.worker = Some(rx);
                }
                self.preview = None;
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (ctx, changed);
        self.key = Some(key);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn snapshot_marks(mut session: solidcraft_engine::Session, request: &Value) -> Option<Marks> {
    // The transient snapshot reads the model. Only the scratch session leaves the drawing.
    session.open_drawing = None;
    let mut reply = session.execute("drawing.view.snapshot", request).ok()?;
    let scene = serde_json::from_value::<Scene>(reply["scene"].take()).ok()?;
    Some(Marks::of(&scene, 0, 0.1))
}

pub(crate) fn on_sheet(
    app: &mut SolveApp,
    painter: &egui::Painter,
    screen: crate::drawing_sheet_page::OnScreen,
    drawing: &Drawing,
    sheet: u64,
    cursor: Option<Vec2>,
    clicked: bool,
) -> bool {
    let Some(mut placement) = app.drawing_workspace.placement.take() else { return false };
    if placement.owner != (app.session.uid, drawing.id, sheet) || placement.parent.is_some_and(|id| drawing.view(id).is_none()) {
        super::close_panel(app);
        return false;
    }
    if let Some((at, beside, request)) = cursor.and_then(|cursor| placement.target(drawing, cursor)) {
        placement.update_preview(app, painter.ctx(), request.clone());
        let t = Tokens::get();
        let point = screen.screen(at);
        if let Some(parent) = placement.parent.and_then(|id| drawing.view_centre(id).ok()) {
            painter.line_segment([screen.screen(parent), point], Stroke::new(1.0, t.accent.gamma_multiply(0.45)));
        }
        if let Some(preview) = &placement.preview {
            screen.paint_preview(painter, preview, at, t.accent);
        }
        let radius = t.space_6;
        for axis in [vec2(radius, 0.0), vec2(0.0, radius)] {
            painter.line_segment([point - axis, point + axis], Stroke::new(1.0, t.accent));
        }
        if clicked {
            let (command, params) = if let (Some(parent), Some(beside)) = (placement.parent, beside) {
                let place = match beside {
                    Beside::Side(side) => json!({"aligned":side.along(at)}),
                    Beside::Diagonal(_) => json!({"at":[at.x,at.y]}),
                };
                ("drawing.view.projected", json!({"parent":encode(parent),"side":beside,"place":place}))
            } else {
                let mut request = request;
                request["sheet"] = json!(encode(sheet));
                request["position_mm"] = json!([at.x, at.y]);
                ("drawing.view.base", request)
            };
            match app.run(command, params) {
                Ok(reply) if placement.parent.is_none() => {
                    placement.parent = reply["view"].as_str().and_then(solidcraft_engine::doc::ids::decode);
                    if placement.parent.is_none() {
                        super::close_panel(app);
                        return true;
                    }
                }
                Ok(_) => {}
                Err(error) => app.set_status(error, true),
            }
            painter.ctx().request_repaint();
        }
    }
    app.drawing_workspace.placement = Some(placement);
    true
}
