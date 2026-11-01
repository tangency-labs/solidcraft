//! Surface analyses: zebra stripes, draft angle and curvature map shading of the model's faces.
//! View state only: no geometry changes and no undo step. The view analyses (these shadings,
//! continuity seams, isocurves and curvature combs) are one group: one at a time is on, and
//! `inspect.analysis` says which or turns it off.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, bool_, num, str_, vec3};
use crate::schema::{TypedContract, output_schema_for, surface_analysis as contract};
use crate::{ContinuitySeam, Result, Session, SurfaceAnalysis, kernel};

const ZEBRA: TypedContract<contract::ZebraParams> = TypedContract::new();
const DRAFT: TypedContract<contract::DraftParams> = TypedContract::new();
const ENVIRONMENT: TypedContract<contract::EnvironmentParams> = TypedContract::new();
const ACCESSIBILITY: TypedContract<contract::AccessibilityParams> = TypedContract::new();
const CURVATURE: TypedContract<contract::CurvatureParams> = TypedContract::new();
const CONTINUITY: TypedContract<contract::ContinuityParams> = TypedContract::new();
/// Most edges one call may pick, and most bodies one call may analyse at once.
const MAX_PICKS: usize = 512;
const MAX_BODIES: usize = 64;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("inspect.zebra", "Zebra Analysis", crate::checked!("inspect.zebra", zebra))
        .contract(&ZEBRA)
        .output(output_schema_for::<contract::AnalysisResult>)
        .at("SKETCH", "INSPECT")
        .icon("zebra")
        .noundo()
        .params("stripes?: number across the view (default 12); clear?: true turns the analysis off"),
    CommandSpec::new("inspect.draft", "Draft Analysis", crate::checked!("inspect.draft", draft))
        .contract(&DRAFT)
        .output(output_schema_for::<contract::AnalysisResult>)
        .at("SKETCH", "INSPECT")
        .icon("draft_analysis")
        .noundo()
        .params("pull?: [x,y,z] direction (default Z); angle?: degrees (default 1); clear?: true turns the analysis off"),
    CommandSpec::new("inspect.environment_map", "Environment Map Analysis", crate::checked!("inspect.environment_map", environment))
        .contract(&ENVIRONMENT)
        .output(output_schema_for::<contract::AnalysisResult>)
        .at("SKETCH", "INSPECT")
        .icon("environment_map")
        .noundo()
        .params("clear?: true turns the analysis off"),
    CommandSpec::new("inspect.accessibility", "Accessibility Analysis", crate::checked!("inspect.accessibility", accessibility))
        .contract(&ACCESSIBILITY)
        .output(output_schema_for::<contract::AnalysisResult>)
        .at("SKETCH", "INSPECT")
        .icon("accessibility")
        .noundo()
        .params("direction?: [x,y,z] the tool comes from (default Z, from above); clear?: true turns the analysis off"),
    CommandSpec::new("inspect.curvature_map", "Curvature Map Analysis", crate::checked!("inspect.curvature_map", curvature_map))
        .contract(&CURVATURE)
        .output(output_schema_for::<contract::AnalysisResult>)
        .at("SKETCH", "INSPECT")
        .icon("curvature_map")
        .noundo()
        .params("radius?: mm shown red (default: a tenth of the model's size); clear?: true turns the analysis off"),
    CommandSpec::new("inspect.analysis", "View Analysis", crate::checked!("inspect.analysis", analysis))
        .icon("zebra")
        .noundo()
        .doc_only()
        .contract(&ANALYSIS_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::surface_analysis::AnalysisState>)
        .params(
            "off?: true turns the view analysis on now off. → active: the command id of the view analysis on (inspect.zebra, \
             inspect.draft, inspect.curvature_map, inspect.environment_map, inspect.accessibility, inspect.continuity, \
             inspect.isocurve or inspect.curvature_comb), or null. One is on at a time: turning one on turns the one on before \
             off. Read-only without `off`",
        ),
    CommandSpec::new("inspect.continuity", "Continuity Analysis", crate::checked!("inspect.continuity", continuity))
        .contract(&CONTINUITY)
        .output(output_schema_for::<contract::ContinuityResult>)
        .at("SURFACE", "INSPECT")
        .icon("continuity")
        .noundo()
        .params(
            "body?: name (default: the visible bodies); edges?: [x,y,z] points or {\"index\": i} (default: every edge of the body; \
             a pick must match one body); samples?: 3…257 stations along each edge, ends included (default 33); \
             gap_tolerance_mm? (default 0.001), angle_tolerance_deg? (default 1), curvature_tolerance_per_mm? (default 0.01), \
             curvature_change_tolerance_per_mm2? (default 0.001), curvature_change_rate_tolerance_per_mm3? (default 0.0001); \
             add?: true keeps the seams already shaded and adds these (measuring an edge again replaces it); \
             clear?: true turns the analysis off. → each edge's worst gap (G0), normal angle (G1), curvature jump (G2), \
             jump of the curvature's change across the seam dκ/ds (G3) and of that change's rate d²κ/ds² (G4), from the \
             faces' exact derivatives, as {value, at: [x,y,z], edge_fraction}; its order (g0…g4: the highest every station \
             meets within the tolerances, null where the faces do not meet), its verdict (smooth: g2 or better, \
             curvature_jump, sharp or gap) and a summary; the seams are shaded in the viewport",
        ),
];

const ANALYSIS_CONTRACT: crate::schema::TypedContract<crate::schema::surface_analysis::AnalysisParams> = crate::schema::TypedContract::new();

/// The view analyses, by the command that turns each on: the shadings, the continuity seams, the
/// isocurves and the curvature combs. One at a time is on.
impl Session {
    /// The view analysis on now, by its command id, or `None`.
    pub fn active_analysis(&self) -> Option<&'static str> {
        if let Some(a) = self.analysis {
            Some(a.command())
        } else if self.comb.is_some() {
            Some("inspect.curvature_comb")
        } else if !self.isocurves.is_empty() {
            Some("inspect.isocurve")
        } else if !self.continuity.is_empty() {
            Some("inspect.continuity")
        } else {
            None
        }
    }

    /// Turn every view analysis off but the one command `keep` turns on (`None`: all of them).
    /// Turning one on leaves the others on while the preference to show several is set
    /// ([`Session::single_surface_analysis`]).
    pub fn analyses_off(&mut self, keep: Option<&str>) {
        if keep.is_some() && !self.single_surface_analysis() {
            return;
        }
        if self.analysis.is_some_and(|a| Some(a.command()) != keep) {
            self.analysis = None;
        }
        if keep != Some("inspect.curvature_comb") {
            self.comb = None;
        }
        if keep != Some("inspect.isocurve") {
            self.isocurves.clear();
        }
        if keep != Some("inspect.continuity") {
            self.continuity.clear();
        }
        self.status_analysis_changed(None);
        self.revision += 1;
    }
}

impl SurfaceAnalysis {
    /// The command that turns this analysis on.
    pub fn command(self) -> &'static str {
        match self {
            SurfaceAnalysis::Zebra { .. } => "inspect.zebra",
            SurfaceAnalysis::Draft { .. } => "inspect.draft",
            SurfaceAnalysis::Curvature { .. } => "inspect.curvature_map",
            SurfaceAnalysis::Environment => "inspect.environment_map",
            SurfaceAnalysis::Access { .. } => "inspect.accessibility",
        }
    }
}

/// `inspect.analysis`: which view analysis is on, and `off` turns it off.
fn analysis(s: &mut Session, p: &Value) -> Result<Value> {
    let off = match p.get("off") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad("inspect.analysis", "`off` must be true or false")),
    };
    if off {
        s.analyses_off(None);
    }
    Ok(json!({"active": s.active_analysis()}))
}

fn environment(s: &mut Session, p: &Value) -> Result<Value> {
    set(s, p, SurfaceAnalysis::Environment)
}

fn set(s: &mut Session, p: &Value, a: SurfaceAnalysis) -> Result<Value> {
    if bool_(p, "clear").unwrap_or(false) {
        s.analysis = None;
    } else {
        s.analyses_off(Some(a.command()));
        s.analysis = Some(a);
    }
    s.status_analysis_changed((!bool_(p, "clear").unwrap_or(false)).then_some(crate::status_items::StatusId::SurfaceAnalysis));
    s.revision += 1;
    let vector = |v: Vec3| [v.x, v.y, v.z];
    let analysis = s.analysis.map(|a| match a {
        SurfaceAnalysis::Zebra { stripes } => contract::Analysis::Zebra { stripes },
        SurfaceAnalysis::Draft { pull, angle } => contract::Analysis::Draft { pull: vector(pull), angle },
        SurfaceAnalysis::Curvature { radius } => contract::Analysis::Curvature { radius },
        SurfaceAnalysis::Environment => contract::Analysis::Environment,
        SurfaceAnalysis::Access { dir } => contract::Analysis::Access { dir: vector(dir) },
    });
    serde_json::to_value(contract::AnalysisResult { analysis, param_warnings: Vec::new() }).map_err(|e| bad("surface analysis", e.to_string()))
}

fn zebra(s: &mut Session, p: &Value) -> Result<Value> {
    let stripes = num(p, "stripes").unwrap_or(12.0);
    if !(stripes.is_finite() && (1.0..=200.0).contains(&stripes)) {
        return Err(bad("inspect.zebra", "`stripes` must be 1…200"));
    }
    set(s, p, SurfaceAnalysis::Zebra { stripes })
}

fn draft(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.draft";
    let pull = match p.get("pull") {
        Some(v) => vec3(v).and_then(Vec3::normalized).ok_or_else(|| bad(cmd, "`pull` must be a non-zero [x,y,z]"))?,
        None => Vec3::Z,
    };
    let angle = num(p, "angle").unwrap_or(1.0);
    if !(angle.is_finite() && (0.0..90.0).contains(&angle)) {
        return Err(bad(cmd, "`angle` must be 0…90 degrees"));
    }
    set(s, p, SurfaceAnalysis::Draft { pull, angle })
}

fn accessibility(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = match p.get("direction") {
        Some(v) => vec3(v).and_then(Vec3::normalized).ok_or_else(|| bad("inspect.accessibility", "`direction` must be a non-zero [x,y,z]"))?,
        None => Vec3::Z,
    };
    set(s, p, SurfaceAnalysis::Access { dir })
}

fn curvature_map(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.curvature_map";
    let radius = match num(p, "radius") {
        Some(r) if r.is_finite() && r > 1e-6 && r < 1e9 => r,
        Some(_) => return Err(bad(cmd, "`radius` must be positive")),
        None => {
            let st = s.model.state();
            let size = st.bodies.iter().map(|b| b.mesh().bounds().diagonal()).fold(0.0_f64, f64::max);
            if size > 0.0 { size / 10.0 } else { 10.0 }
        }
    };
    set(s, p, SurfaceAnalysis::Curvature { radius })
}

/// `inspect.continuity`: measure how far the two faces meeting at each picked edge (or at every
/// edge of a body) depart from sharing a curve (G0), a tangent plane (G1) and a curvature (G2),
/// and keep the seams so the viewport can shade them. View state only: the design and its undo
/// history are untouched (`noundo`), as with the other analyses.
fn continuity(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.continuity";
    if bool_(p, "clear").unwrap_or(false) {
        s.continuity.clear();
        s.status_analysis_changed(None);
        s.revision += 1;
        return serde_json::to_value(contract::ContinuityResult::Cleared {
            analysis: "continuity".into(),
            cleared: true,
            param_warnings: Vec::new(),
        })
        .map_err(|e| bad(cmd, e.to_string()));
    }
    let samples = match p.get("samples") {
        None => kernel::DEFAULT_SAMPLES,
        Some(v) => match v.as_f64().filter(|n| n.is_finite() && n.fract() == 0.0 && (3.0..=257.0).contains(n)) {
            Some(n) => n as usize,
            None => return Err(bad(cmd, "`samples` must be 3…257 stations along each edge")),
        },
    };
    let default = kernel::ContinuityTolerances::default();
    let tolerances = kernel::ContinuityTolerances {
        gap_mm: tolerance(p, "gap_tolerance_mm", default.gap_mm)?,
        angle_deg: tolerance(p, "angle_tolerance_deg", default.angle_deg)?,
        curvature_per_mm: tolerance(p, "curvature_tolerance_per_mm", default.curvature_per_mm)?,
        curvature_change_per_mm2: tolerance(p, "curvature_change_tolerance_per_mm2", default.curvature_change_per_mm2)?,
        curvature_change_rate_per_mm3: tolerance(p, "curvature_change_rate_tolerance_per_mm3", default.curvature_change_rate_per_mm3)?,
    };
    let picks = edge_picks(p)?;
    let st = s.world_state()?;
    let named = str_(p, "body");
    let candidates: Vec<&solidcraft_doc::ModelBody> = match named {
        Some(name) => vec![st.body(name).ok_or_else(|| bad(cmd, format!("no body `{name}`")))?],
        None => st.bodies.iter().filter(|b| s.body_visible(&b.name)).collect(),
    };
    if candidates.is_empty() {
        return Err(bad(cmd, "there are no visible bodies to analyse: name a `body`"));
    }
    if candidates.len() > MAX_BODIES {
        return Err(bad(cmd, format!("more than {MAX_BODIES} bodies are shown: name the `body` to analyse")));
    }
    let mut targets: Vec<(&solidcraft_doc::ModelBody, Vec<usize>)> = Vec::new();
    if picks.is_empty() {
        targets.extend(candidates.into_iter().map(|b| (b, Vec::new())));
    } else {
        // Picks mean edges of one body (a design may stack bodies at the same place): a list
        // that lands on one body's edges only is analysed, and anything else is refused.
        let mut hits = Vec::new();
        let mut refused = None;
        for b in candidates {
            match kernel::pick_edges(&b.body, &picks) {
                Ok(edges) => hits.push((b, edges)),
                Err(e) => refused = refused.or(Some(e)),
            }
        }
        if hits.is_empty() {
            return Err(match (named, refused) {
                (Some(_), Some(e)) => e.into(),
                _ => bad(cmd, "no visible body has an edge matching `edges`"),
            });
        }
        if hits.len() > 1 {
            return Err(bad(cmd, "`edges` matches edges of more than one body: name the `body` to analyse"));
        }
        targets.extend(hits);
    }
    let mut seams: Vec<ContinuitySeam> = Vec::new();
    let mut bodies = Vec::new();
    let mut summary = Summary::default();
    for (body, edges) in &targets {
        let report = kernel::seam_diagnostics(&body.body, edges, samples, tolerances)?;
        let mut rows = Vec::new();
        for seam in &report.seams {
            summary.count(seam.verdict, seam.order);
            seams.push(ContinuitySeam {
                body: body.name.clone(),
                edge: seam.edge,
                points: seam.points.clone(),
                verdict: seam.verdict,
                order: seam.order,
                details: crate::status_details::ContinuityDetail {
                    body: body.name.clone(),
                    edge: seam.edge,
                    samples: seam.samples,
                    gap_tolerance_mm: tolerances.gap_mm,
                    angle_tolerance_deg: tolerances.angle_deg,
                    curvature_tolerance_per_mm: tolerances.curvature_per_mm,
                    gap_mm: seam.gap_mm.value,
                    normal_angle_deg: seam.normal_angle_deg.value,
                    curvature_jump_per_mm: seam.curvature_jump_per_mm.value,
                    length_mm: seam.length,
                    verdict: seam.verdict.into(),
                    gap_at: [seam.gap_mm.at.x, seam.gap_mm.at.y, seam.gap_mm.at.z],
                    gap_fraction: seam.gap_mm.edge_fraction,
                    normal_angle_at: [seam.normal_angle_deg.at.x, seam.normal_angle_deg.at.y, seam.normal_angle_deg.at.z],
                    normal_angle_fraction: seam.normal_angle_deg.edge_fraction,
                    curvature_jump_at: [seam.curvature_jump_per_mm.at.x, seam.curvature_jump_per_mm.at.y, seam.curvature_jump_per_mm.at.z],
                    curvature_jump_fraction: seam.curvature_jump_per_mm.edge_fraction,
                    skipped: report.skipped,
                    unmeasured: report.unmeasured,
                    current: true,
                    geometry_key: body.geometry_key(),
                    feature: body.feature,
                },
            });
            rows.push(contract::EdgeReport {
                edge: seam.edge,
                length_mm: seam.length,
                samples: seam.samples,
                gap_mm: metric(seam.gap_mm),
                normal_angle_deg: metric(seam.normal_angle_deg),
                curvature_jump_per_mm: metric(seam.curvature_jump_per_mm),
                curvature_change_jump_per_mm2: metric(seam.curvature_change_jump_per_mm2),
                curvature_change_rate_jump_per_mm3: metric(seam.curvature_change_rate_jump_per_mm3),
                order: seam.order.map(|g| g.as_str().to_string()),
                verdict: verdict(seam.verdict),
            });
        }
        summary.skipped += report.skipped;
        summary.unmeasured += report.unmeasured;
        bodies.push(contract::BodyReport { body: body.name.clone(), edges: rows, skipped: report.skipped, unmeasured: report.unmeasured });
    }
    if bool_(p, "add").unwrap_or(false) {
        // The seams shown stay; an edge measured again takes its new reading.
        s.continuity.retain(|kept| !seams.iter().any(|new| new.body == kept.body && new.edge == kept.edge));
        s.continuity.extend(seams);
    } else {
        s.continuity = seams;
    }
    if !s.continuity.is_empty() {
        s.analyses_off(Some("inspect.continuity"));
    }
    s.status_analysis_changed(Some(crate::status_items::StatusId::Continuity));
    s.revision += 1;
    serde_json::to_value(contract::ContinuityResult::Measured {
        analysis: "continuity".into(),
        samples,
        tolerances: contract::Tolerances {
            gap_mm: tolerances.gap_mm,
            angle_deg: tolerances.angle_deg,
            curvature_per_mm: tolerances.curvature_per_mm,
            curvature_change_per_mm2: tolerances.curvature_change_per_mm2,
            curvature_change_rate_per_mm3: tolerances.curvature_change_rate_per_mm3,
        },
        bodies,
        summary: contract::Summary {
            edges: summary.edges,
            smooth: summary.smooth,
            curvature_jump: summary.curvature_jump,
            sharp: summary.sharp,
            gap: summary.gap,
            skipped: summary.skipped,
            unmeasured: summary.unmeasured,
            orders: summary.orders(),
            worst: summary.worst().map(verdict),
        },
        param_warnings: Vec::new(),
    })
    .map_err(|e| bad(cmd, e.to_string()))
}

fn verdict(value: kernel::SeamVerdict) -> contract::Verdict {
    match value {
        kernel::SeamVerdict::Smooth => contract::Verdict::Smooth,
        kernel::SeamVerdict::CurvatureJump => contract::Verdict::CurvatureJump,
        kernel::SeamVerdict::Sharp => contract::Verdict::Sharp,
        kernel::SeamVerdict::Gap => contract::Verdict::Gap,
    }
}
fn metric(value: kernel::SeamWorst) -> contract::Metric {
    contract::Metric { value: value.value, at: [value.at.x, value.at.y, value.at.z], edge_fraction: value.edge_fraction }
}

/// A tolerance parameter. A value that is given but unusable (NaN, infinity, negative, a
/// string, a bare `null`) is refused: `num` would fold it into "not given".
fn tolerance(p: &Value, key: &str, default: f64) -> Result<f64> {
    match p.get(key) {
        None => Ok(default),
        Some(v) => match v.as_f64() {
            Some(x) if x.is_finite() && (0.0..1e9).contains(&x) => Ok(x),
            _ => Err(bad("inspect.continuity", format!("`{key}` must be a finite number, 0 or more"))),
        },
    }
}

/// The `edges` parameter: points on the edges to measure, or `{"index": i}` entries in the
/// display mesh's edge order. Absent (or an empty list) means every edge of the body.
fn edge_picks(p: &Value) -> Result<Vec<kernel::EdgePick>> {
    let Some(value) = p.get("edges") else { return Ok(Vec::new()) };
    let Some(list) = value.as_array() else {
        return Err(bad("inspect.continuity", "`edges` must be a list of [x,y,z] points or {\"index\": i} entries"));
    };
    if list.len() > MAX_PICKS {
        return Err(bad("inspect.continuity", format!("pick at most {MAX_PICKS} edges")));
    }
    let mut out = Vec::new();
    for item in list {
        if let Some(index) = item.get("index").and_then(Value::as_u64) {
            let index = usize::try_from(index).map_err(|_| bad("inspect.continuity", "`index` is too large"))?;
            out.push(kernel::EdgePick::Index(index));
        } else if let Some(point) = vec3(item) {
            out.push(kernel::EdgePick::Point(point));
        } else {
            return Err(bad("inspect.continuity", "each `edges` entry is a point [x,y,z] or {\"index\": i}"));
        }
    }
    Ok(out)
}

/// Counts of a continuity reply, and the worst verdict over every edge measured.
#[derive(Clone, Copy, Debug, Default)]
struct Summary {
    edges: usize,
    smooth: usize,
    curvature_jump: usize,
    sharp: usize,
    gap: usize,
    skipped: usize,
    unmeasured: usize,
    /// Seams by the highest order they meet, indexed by [`kernel::Continuity::order`].
    by_order: [usize; 5],
}

impl Summary {
    fn count(&mut self, verdict: kernel::SeamVerdict, order: Option<kernel::Continuity>) {
        self.edges += 1;
        if let Some(slot) = order.and_then(|g| self.by_order.get_mut(g.order())) {
            *slot += 1;
        }
        match verdict {
            kernel::SeamVerdict::Smooth => self.smooth += 1,
            kernel::SeamVerdict::CurvatureJump => self.curvature_jump += 1,
            kernel::SeamVerdict::Sharp => self.sharp += 1,
            kernel::SeamVerdict::Gap => self.gap += 1,
        }
    }

    /// How many seams meet each order at best, by its word (`g0` … `g4`).
    fn orders(&self) -> serde_json::Map<String, Value> {
        kernel::Continuity::ALL.into_iter().zip(self.by_order).map(|(g, n)| (g.as_str().to_string(), json!(n))).collect()
    }

    /// The worst verdict over the edges measured (`None` when there was nothing to measure).
    /// [`kernel::SeamVerdict`] runs from the best continuity to the worst.
    fn worst(&self) -> Option<kernel::SeamVerdict> {
        [
            (self.smooth, kernel::SeamVerdict::Smooth),
            (self.curvature_jump, kernel::SeamVerdict::CurvatureJump),
            (self.sharp, kernel::SeamVerdict::Sharp),
            (self.gap, kernel::SeamVerdict::Gap),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(_, verdict)| verdict)
        .max()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::{Session, SurfaceAnalysis};

    fn boxed(s: &mut Session, size: f64) {
        s.execute("solid.box", &json!({"length": size, "width": size, "height": size})).unwrap();
    }

    fn status(s: &mut Session) -> Vec<crate::status_items::StatusItem> {
        serde_json::from_value::<crate::status_items::StatusItems>(s.execute("view.status_items", &json!({})).unwrap()).unwrap().items
    }

    #[test]
    fn newest_status_uses_activation_not_row_order_and_filters_unpresented_effects() {
        use crate::status_items::StatusId::{Continuity, Section, SurfaceAnalysis};
        let mut s = Session::default();
        s.execute("inspect.zebra", &json!({})).unwrap();
        s.execute("inspect.section", &json!({"plane":"XY"})).unwrap();
        s.execute("inspect.draft", &json!({"angle":7})).unwrap();
        assert_eq!(status(&mut s).iter().map(|item| item.id).collect::<Vec<_>>(), [SurfaceAnalysis, Section]);
        assert_eq!(s.newest_status_item(&[SurfaceAnalysis, Continuity, Section]).unwrap().command, "inspect.draft");
        s.execute("inspect.section", &json!({"plane":"XZ"})).unwrap();
        assert_eq!(s.newest_status_item(&[SurfaceAnalysis, Continuity, Section]).unwrap().id, Section);
        let before = (s.revision, s.undo.len(), s.doc.to_json(), s.status_activation.clone());
        assert_eq!(s.newest_status_item(&[SurfaceAnalysis, Continuity]).unwrap().command, "inspect.draft");
        assert_eq!((s.revision, s.undo.len(), s.doc.to_json(), s.status_activation.clone()), before);
        assert!(s.execute("inspect.zebra", &json!({"stripes":-1})).is_err());
        assert_eq!(s.status_activation, before.3);
        s.execute("inspect.draft", &json!({"clear":true})).unwrap();
        assert!(s.newest_status_item(&[SurfaceAnalysis, Continuity]).is_none());
        s.execute("inspect.zebra", &json!({})).unwrap();
        assert_eq!(s.newest_status_item(&[SurfaceAnalysis, Continuity, Section]).unwrap().command, "inspect.zebra");
    }

    #[test]
    fn newest_status_tracks_independent_continuity_and_policy_normalisation() {
        use crate::status_items::StatusId::{Continuity, SurfaceAnalysis};
        for continuity_first in [false, true] {
            let mut s = Session::default();
            boxed(&mut s, 10.0);
            s.set_single_surface_analysis(false);
            let shading = ("inspect.zebra", json!({}));
            let seams = ("inspect.continuity", json!({"edges":[[5,0,0]],"samples":5}));
            for (cmd, params) in if continuity_first { [seams, shading] } else { [shading, seams] } {
                s.execute(cmd, &params).unwrap();
            }
            let latest = if continuity_first { "inspect.zebra" } else { "inspect.continuity" };
            assert_eq!(s.newest_status_item(&[SurfaceAnalysis, Continuity]).unwrap().command, latest);
            s.set_single_surface_analysis(true);
            assert_eq!(s.newest_status_item(&[SurfaceAnalysis]).unwrap().command, latest);
            s.set_single_surface_analysis(false);
            let clear = s.newest_status_item(&[SurfaceAnalysis, Continuity]).unwrap().clear;
            s.execute(&clear.command, &json!({"clear":true})).unwrap();
            assert!(s.newest_status_item(&[SurfaceAnalysis, Continuity]).is_none());
        }
    }

    #[test]
    fn status_items_zero_seam_continuity_never_leaves_a_phantom_winner() {
        use crate::status_items::StatusId;
        for single in [false, true] {
            let mut empty = Session::default();
            empty.set_single_surface_analysis(single);
            empty.execute("inspect.zebra", &json!({"stripes":14.5})).unwrap();
            let before = (status(&mut empty), empty.latest_analysis, empty.revision);
            assert!(empty.execute("inspect.continuity", &json!({})).is_err());
            assert_eq!((status(&mut empty), empty.latest_analysis, empty.revision), before, "empty-model failure is atomic");
            for had_continuity in [false, true] {
                let mut s = Session::default();
                s.set_single_surface_analysis(single);
                boxed(&mut s, 10.0);
                for (command, params) in [
                    ("sketch.create", json!({"plane":"XY","name":"Boundary"})),
                    ("sketch.rectangle.two_point", json!({"p0":[20,20],"p1":[30,30]})),
                    ("sketch.finish", json!({})),
                    ("surface.patch", json!({"sketch":"Boundary","body_name":"Sheet"})),
                    ("inspect.zebra", json!({"stripes":14.5})),
                    ("inspect.section", json!({"plane":"XY"})),
                ] {
                    s.execute(command, &params).unwrap();
                }
                if had_continuity {
                    s.execute("inspect.continuity", &json!({"body":"Body1","edges":[[5,0,0]],"samples":5})).unwrap();
                    assert_eq!(s.continuity.len(), 1);
                    if single {
                        s.execute("inspect.zebra", &json!({"stripes":14.5})).unwrap();
                    }
                }
                s.mark_saved();
                let doc = s.doc.clone();
                let undo = s.undo.len();
                let result = s.execute("inspect.continuity", &json!({"body":"Sheet","samples":5})).unwrap();
                assert_eq!(result["summary"]["edges"], 0);
                assert_eq!(result["summary"]["skipped"], 4, "an actual one-face sheet has four open boundaries");
                assert!(s.continuity.is_empty());
                assert_eq!(s.latest_analysis, if single { None } else { Some(StatusId::SurfaceAnalysis) });
                if single {
                    assert_eq!(s.analysis, None, "single-mode successful zero-result replacement retains its existing clear semantics");
                } else {
                    assert_eq!(s.analysis, Some(SurfaceAnalysis::Zebra { stripes: 14.5 }), "legacy zero result preserves shading");
                }
                let expected = if single { vec![StatusId::Section] } else { vec![StatusId::SurfaceAnalysis, StatusId::Section] };
                for policy in [true, false, true] {
                    s.set_single_surface_analysis(policy);
                    assert_eq!(status(&mut s).iter().map(|item| item.id).collect::<Vec<_>>(), expected);
                }
                assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
                assert!(!s.is_dirty());
                assert_eq!(s.undo.len(), undo);
            }
        }
    }

    #[test]
    fn status_items_legacy_policy_keeps_independent_effects_and_clears_in_both_orders() {
        use crate::status_items::StatusId::{Continuity, Section, SurfaceAnalysis};
        for continuity_first in [false, true] {
            let mut s = Session::default();
            boxed(&mut s, 10.0);
            s.mark_saved();
            let doc = s.doc.clone();
            let undo = s.undo.len();
            s.set_single_surface_analysis(false);
            let shading = ("inspect.zebra", json!({"stripes": 14.5}));
            let continuity = ("inspect.continuity", json!({"edges": [[5, 0, 0]], "samples": 5}));
            for (command, params) in if continuity_first { [continuity, shading] } else { [shading, continuity] } {
                s.execute(command, &params).unwrap();
            }
            s.execute("inspect.section", &json!({"plane": "XY"})).unwrap();
            let items = status(&mut s);
            let expected = if continuity_first { vec![Continuity, SurfaceAnalysis, Section] } else { vec![SurfaceAnalysis, Continuity, Section] };
            assert_eq!(items.iter().map(|item| item.id).collect::<Vec<_>>(), expected);
            assert_eq!(s.analysis, Some(crate::SurfaceAnalysis::Zebra { stripes: 14.5 }));
            assert_eq!(s.continuity.len(), 1);
            let before = (items.clone(), s.status_order.clone(), s.latest_analysis, s.revision, s.continuity.clone());
            for (command, params) in [
                ("inspect.continuity", json!({"body": "missing"})),
                ("inspect.draft", json!({"pull": [0, 0, 0]})),
                ("inspect.zebra", json!({"stripes": -2})),
            ] {
                assert!(s.execute(command, &params).is_err());
                assert_eq!((status(&mut s), s.status_order.clone(), s.latest_analysis, s.revision, s.continuity.clone()), before);
            }
            // The legacy generic shading clear still ends Zebra, never Continuity or Section.
            s.execute("inspect.curvature_map", &json!({"clear": true})).unwrap();
            assert_eq!(status(&mut s).iter().map(|item| item.id).collect::<Vec<_>>(), vec![Continuity, Section]);
            let clear = items.iter().find(|item| item.id == Continuity).unwrap().clear.clone();
            for _ in 0..2 {
                s.execute(&clear.command, &serde_json::to_value(&clear.params).unwrap()).unwrap();
                assert_eq!(status(&mut s).iter().map(|item| item.id).collect::<Vec<_>>(), vec![Section]);
            }
            // Clear in the opposite direction leaves the shading active too.
            s.execute("inspect.zebra", &json!({})).unwrap();
            s.execute("inspect.continuity", &json!({"edges": [[5, 0, 0]]})).unwrap();
            s.execute("inspect.continuity", &json!({"clear": true})).unwrap();
            assert!(s.analysis.is_some());
            assert!(s.section.is_some());
            assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
            assert!(!s.is_dirty());
            assert_eq!(s.undo.len(), undo);
        }
    }

    #[test]
    fn status_items_policy_switch_keeps_latest_replacement_at_oldest_group_position() {
        use crate::status_items::{StatusId, StatusSetting};
        for continuity_first in [false, true] {
            for latest_continuity in [false, true] {
                let mut s = Session::default();
                boxed(&mut s, 10.0);
                s.mark_saved();
                let doc = s.doc.clone();
                let undo = s.undo.len();
                s.set_single_surface_analysis(false);
                let shading = ("inspect.zebra", json!({}));
                let continuity = ("inspect.continuity", json!({"edges": [[5, 0, 0]], "samples": 5}));
                let (first, last) = if continuity_first { (continuity, shading) } else { (shading, continuity) };
                s.execute(first.0, &first.1).unwrap();
                s.execute("inspect.section", &json!({"plane": "XY"})).unwrap();
                s.execute(last.0, &last.1).unwrap();
                // Replacement does not move the slot, but determines the policy-switch winner.
                if latest_continuity {
                    s.execute("inspect.continuity", &json!({"edges": [[0, 5, 0]], "add": true})).unwrap();
                } else {
                    s.execute("inspect.draft", &json!({"angle": 7.5})).unwrap();
                }
                let before = status(&mut s);
                for _ in 0..3 {
                    assert_eq!(status(&mut s), before);
                }
                s.set_single_surface_analysis(true);
                let items = status(&mut s);
                assert_eq!(items.iter().map(|item| item.id).collect::<Vec<_>>(), vec![StatusId::SurfaceAnalysis, StatusId::Section]);
                assert_eq!(items[0].command, if latest_continuity { "inspect.continuity" } else { "inspect.draft" });
                if latest_continuity {
                    assert_eq!(s.analysis, None);
                    assert_eq!(items[0].setting, StatusSetting::Continuity { edge_count: 2 });
                } else {
                    assert!(s.continuity.is_empty());
                    assert!(matches!(s.analysis, Some(SurfaceAnalysis::Draft { angle, .. }) if angle == 7.5));
                }
                let revision = s.revision;
                s.set_single_surface_analysis(true);
                assert_eq!(s.revision, revision, "idempotent policy application");
                s.set_single_surface_analysis(false);
                assert_eq!(status(&mut s)[0].id, if latest_continuity { StatusId::Continuity } else { StatusId::SurfaceAnalysis });
                assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
                assert!(!s.is_dirty());
                assert_eq!(s.undo.len(), undo);
                assert!(!s.scratch().single_surface_analysis());
                s.execute("file.new", &json!({})).unwrap();
                assert!(!s.single_surface_analysis(), "new/open retain SDK session configuration");
                assert!(status(&mut s).is_empty());
            }
        }
    }

    #[test]
    fn status_items_describe_every_effect_and_execute_their_own_clear() {
        use crate::status_items::{StatusId, StatusKind, StatusSetting};
        let mut s = Session::default();
        assert!(status(&mut s).is_empty());
        for (command, params, setting, has_key) in [
            ("inspect.zebra", json!({"stripes": 12.5}), StatusSetting::Zebra { stripes: 12.5 }, false),
            ("inspect.draft", json!({"pull": [0, -2, 0], "angle": 3.25}), StatusSetting::Draft { pull: [0.0, -1.0, 0.0], angle_deg: 3.25 }, true),
            ("inspect.environment_map", json!({}), StatusSetting::Environment, false),
            ("inspect.accessibility", json!({"direction": [-3, 0, 0]}), StatusSetting::Accessibility { direction: [-1.0, 0.0, 0.0] }, true),
            ("inspect.curvature_map", json!({"radius": 2.75}), StatusSetting::Curvature { radius_mm: 2.75 }, true),
        ] {
            s.execute(command, &params).unwrap();
            let items = status(&mut s);
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item.id, StatusId::SurfaceAnalysis);
            assert_eq!(item.kind, StatusKind::Mode);
            assert_eq!(item.command, command);
            assert_eq!(item.setting, setting);
            assert_eq!(item.has_key, has_key);
            assert_eq!(item.clear.command, command);
            let params = serde_json::to_value(&item.clear.params).unwrap();
            for _ in 0..2 {
                s.execute(&item.clear.command, &params).unwrap();
                assert!(status(&mut s).is_empty());
            }
        }
        for (params, origin, normal, hatch) in [
            (json!({"plane": "XY", "at": 20, "hatch": false}), [0.0, 0.0, 20.0], [0.0, 0.0, 1.0], false),
            (json!({"plane": {"origin": [1, 2, 3], "normal": [0, 3, 4]}}), [1.0, 2.0, 3.0], [0.0, 0.6, 0.8], true),
        ] {
            s.execute("inspect.section", &params).unwrap();
            let item = status(&mut s).remove(0);
            assert_eq!(item.id, StatusId::Section);
            assert_eq!(item.kind, StatusKind::Mode);
            assert_eq!(item.command, "inspect.section");
            assert_eq!(item.setting, StatusSetting::Section { origin_mm: origin, normal, hatch });
            assert!(!item.has_key);
            s.execute(&item.clear.command, &serde_json::to_value(item.clear.params).unwrap()).unwrap();
            assert!(status(&mut s).is_empty());
        }
    }

    #[test]
    fn status_items_order_slots_by_activation_and_retain_replacement_position() {
        use crate::status_items::StatusId::{Section, SurfaceAnalysis};
        for section_first in [false, true] {
            let mut s = Session::default();
            let section = ("inspect.section", json!({"plane": "XY"}));
            let zebra = ("inspect.zebra", json!({}));
            for (command, params) in if section_first { [section, zebra] } else { [zebra, section] } {
                s.execute(command, &params).unwrap();
            }
            let expected = if section_first { vec![Section, SurfaceAnalysis] } else { vec![SurfaceAnalysis, Section] };
            assert_eq!(status(&mut s).iter().map(|item| item.id).collect::<Vec<_>>(), expected);
            s.execute("inspect.draft", &json!({"angle": 7})).unwrap();
            s.execute("inspect.section", &json!({"plane": "YZ", "at": 9})).unwrap();
            assert_eq!(
                status(&mut s).iter().map(|item| item.id).collect::<Vec<_>>(),
                expected,
                "analysis and Section replacements retain their slots"
            );
            s.execute("inspect.draft", &json!({"clear": true})).unwrap();
            assert_eq!(status(&mut s).iter().map(|item| item.id).collect::<Vec<_>>(), vec![Section]);
            s.execute("inspect.zebra", &json!({})).unwrap();
            assert_eq!(status(&mut s).iter().map(|item| item.id).collect::<Vec<_>>(), vec![Section, SurfaceAnalysis]);
            s.execute("inspect.section", &json!({"clear": true})).unwrap();
            assert_eq!(status(&mut s).len(), 1);
            s.execute("inspect.section", &json!({"plane": "XZ"})).unwrap();
            assert_eq!(status(&mut s).iter().map(|item| item.id).collect::<Vec<_>>(), vec![SurfaceAnalysis, Section]);
        }
    }

    #[test]
    fn status_items_continuity_shares_the_analysis_slot_and_failures_keep_effects() {
        use crate::status_items::{StatusId, StatusSetting};
        let mut s = Session::default();
        boxed(&mut s, 10.0);
        s.execute("inspect.zebra", &json!({})).unwrap();
        s.execute("inspect.section", &json!({"plane": "XY"})).unwrap();
        s.execute("inspect.continuity", &json!({"edges": [[5, 0, 0]]})).unwrap();
        assert_eq!(s.analysis, None);
        let continuity = status(&mut s)[0].clone();
        assert_eq!(continuity.id, StatusId::SurfaceAnalysis);
        assert_eq!(continuity.command, "inspect.continuity");
        assert_eq!(continuity.setting, StatusSetting::Continuity { edge_count: 1 });
        assert!(!continuity.has_key);
        s.execute("inspect.continuity", &json!({"edges": [[0, 5, 0]], "add": true})).unwrap();
        assert_eq!(status(&mut s)[0].setting, StatusSetting::Continuity { edge_count: 2 });
        let before = status(&mut s);
        let order = s.status_order.clone();
        let effects = (s.analysis, s.continuity.clone(), s.section, s.section_hatch, s.revision);
        for (command, params) in [
            ("inspect.continuity", json!({"body": "missing"})),
            ("inspect.continuity", json!({"samples": 2})),
            ("inspect.draft", json!({"pull": [0, 0, 0]})),
            ("inspect.zebra", json!({"stripes": -1})),
            ("inspect.section", json!({"plane": {"normal": [0, 0, 0]}})),
        ] {
            assert!(s.execute(command, &params).is_err());
            assert_eq!(status(&mut s), before);
            assert_eq!(s.status_order, order);
            assert_eq!((s.analysis, s.continuity.clone(), s.section, s.section_hatch, s.revision), effects);
        }
        s.execute("inspect.draft", &json!({})).unwrap();
        assert!(s.continuity.is_empty());
        let before = status(&mut s);
        assert!(s.execute("inspect.continuity", &json!({"body": "missing"})).is_err());
        assert_eq!(status(&mut s), before, "failed Continuity activation keeps surface shading and ordering");
        s.execute(&continuity.clear.command, &serde_json::to_value(continuity.clear.params).unwrap()).unwrap();
        assert_eq!(status(&mut s)[0].command, "inspect.draft", "stale continuity clear keeps replacement");
        s.execute("inspect.continuity", &json!({"edges": [[5, 0, 0]]})).unwrap();
        let item = status(&mut s).remove(0);
        s.execute(&item.clear.command, &serde_json::to_value(item.clear.params).unwrap()).unwrap();
        assert_eq!(status(&mut s).len(), 1);
        assert_eq!(status(&mut s)[0].id, StatusId::Section);
    }

    #[test]
    fn status_items_queries_are_read_only_even_during_a_stale_live_drag() {
        use std::sync::Arc;
        let mut s = Session::default();
        for (command, params) in [
            ("sketch.create", json!({"plane":"XY"})),
            ("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[25,20]})),
            ("sketch.dimension", json!({"entities":["l1"],"value":20})),
            ("sketch.dimension", json!({"entities":["l2"],"value":15})),
            ("sketch.finish", json!({})),
            ("solid.extrude", json!({"distance":5})),
            ("inspect.zebra", json!({"stripes": 10.5})),
            ("inspect.section", json!({"plane":"XY", "at": 2})),
            ("sketch.edit", json!({"sketch":"Sketch1"})),
        ] {
            s.execute(command, &params).unwrap();
        }
        let effects = status(&mut s);
        s.execute("sketch.drag", &json!({"entity":"l1.start","from":[5,5],"to":[4,5],"live":true})).unwrap();
        assert!(s.model.is_stale());
        let doc = s.doc.clone();
        let model = s.model.state();
        let selection = s.selection.clone();
        let before = (s.revision, s.undo.len(), s.redo.len(), s.is_dirty(), s.evaluations, s.status_order.clone());
        for _ in 0..3 {
            assert_eq!(status(&mut s), effects);
        }
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert!(Arc::ptr_eq(&model, &s.model.state()));
        assert!(s.model.is_stale());
        assert_eq!(s.selection, selection);
        assert_eq!((s.revision, s.undo.len(), s.redo.len(), s.is_dirty(), s.evaluations, s.status_order.clone()), before);
        assert!(s.scratch().status_items().is_empty(), "scratch sessions have no persistent view effects");
        s.execute("file.new", &json!({})).unwrap();
        assert!(status(&mut s).is_empty());
        assert!(s.status_order.is_empty(), "new design resets transient chronology");
    }

    #[test]
    fn continuity_details_never_validate_old_matching_geometry_during_a_live_drag() {
        use std::sync::Arc;
        let mut s = Session::default();
        for (command, params) in [
            ("sketch.create", json!({"plane":"XY"})),
            ("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[25,20]})),
            ("sketch.dimension", json!({"entities":["l1"],"value":20})),
            ("sketch.dimension", json!({"entities":["l2"],"value":15})),
            ("sketch.finish", json!({})),
            ("solid.extrude", json!({"distance":5})),
            ("inspect.continuity", json!({"samples":5})),
            ("sketch.edit", json!({"sketch":"Sketch1"})),
        ] {
            s.execute(command, &params).unwrap();
        }
        let records = s.continuity.clone();
        assert!(!records.is_empty());
        assert!(s.continuity_details().iter().all(|row| row.current));
        let before_drag = s.model.state();
        s.execute("sketch.drag", &json!({"entity":"l1.start","from":[5,5],"to":[4,5],"live":true})).unwrap();
        assert!(s.model.is_stale());
        // The live solver replaces ModelState to show the moved sketch, while retaining
        // its old downstream B-rep body. That body/key, not the whole container Arc,
        // is the misleading geometry the pure descriptor must refuse.
        let retained_model = s.model.state();
        assert!(before_drag.body(&records[0].body).unwrap().body.same_shape(&retained_model.body(&records[0].body).unwrap().body));
        assert_eq!(
            retained_model.body(&records[0].body).unwrap().geometry_key(),
            records[0].details.geometry_key,
            "this real pending edit retains exactly the misleading old matching geometry key"
        );
        let document = s.doc.clone();
        let before = (s.revision, s.undo.len(), s.redo.len(), s.is_dirty(), s.evaluations, s.status_order.clone());
        for _ in 0..3 {
            let reply = s.execute("view.status_details", &json!({})).unwrap();
            assert!(reply["continuity"].as_array().unwrap().iter().all(|row| row["current"] == false));
            assert!(s.continuity_target_points(&records[0].body, records[0].edge).is_none());
        }
        assert!(s.model.is_stale());
        assert!(Arc::ptr_eq(&document, &s.doc));
        assert!(Arc::ptr_eq(&retained_model, &s.model.state()));
        assert_eq!(s.continuity, records, "query does not clean or fabricate measurements");
        assert_eq!((s.revision, s.undo.len(), s.redo.len(), s.is_dirty(), s.evaluations, s.status_order.clone()), before);
        s.execute("sketch.drag_end", &json!({})).unwrap();
        s.execute("sketch.finish", &json!({})).unwrap();
        s.execute("inspect.continuity", &json!({"samples":5})).unwrap();
        assert!(!s.model.is_stale());
        assert!(s.continuity_details().iter().all(|row| row.current));
        let fresh = &s.continuity[0];
        assert!(s.continuity_target_points(&fresh.body, fresh.edge).is_some(), "normal fresh evaluation/measurement restores valid targets");
    }

    /// The edges of the first body of a continuity reply.
    fn edges(v: &Value) -> &Vec<Value> {
        v["bodies"][0]["edges"].as_array().unwrap_or_else(|| panic!("no edges in {v}"))
    }

    /// The seams that read as tangent (within 0.01° of the neighbouring face).
    fn tangent(edges: &[Value]) -> Vec<&Value> {
        edges.iter().filter(|e| e["normal_angle_deg"]["value"].as_f64().unwrap_or(f64::NAN) < 0.01).collect()
    }

    #[test]
    fn surface_analyses_are_view_state() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let undo = s.undo.len();
        s.execute("inspect.zebra", &json!({"stripes": 20})).unwrap();
        assert_eq!(s.analysis, Some(SurfaceAnalysis::Zebra { stripes: 20.0 }));
        s.execute("inspect.draft", &json!({"pull": [0, 0, 2], "angle": 3})).unwrap();
        assert!(matches!(s.analysis, Some(SurfaceAnalysis::Draft { angle, .. }) if angle == 3.0));
        s.execute("inspect.curvature_map", &json!({})).unwrap();
        assert!(matches!(s.analysis, Some(SurfaceAnalysis::Curvature { radius }) if (radius - 300f64.sqrt() / 10.0).abs() < 1e-6));
        s.execute("inspect.environment_map", &json!({})).unwrap();
        assert_eq!(s.analysis, Some(SurfaceAnalysis::Environment));
        s.execute("inspect.accessibility", &json!({"direction": [0, 0, -3]})).unwrap();
        assert_eq!(s.analysis, Some(SurfaceAnalysis::Access { dir: solidcraft_geom::Vec3::new(0.0, 0.0, -1.0) }));
        s.execute("inspect.curvature_map", &json!({"clear": true})).unwrap();
        assert_eq!(s.analysis, None);
        assert_eq!(s.undo.len(), undo, "no undo steps");
        assert!(s.execute("inspect.zebra", &json!({"stripes": 0})).is_err());
        assert!(s.execute("inspect.draft", &json!({"pull": [0, 0, 0]})).is_err());
    }

    /// A box edge: the faces meet exactly (G0) and square to each other (G1), so the analysis
    /// must read 0 gap and 90°, and must keep its seams as view state without an undo step.
    #[test]
    fn continuity_reports_a_box_edge_as_a_gap_free_right_angle() {
        let mut s = Session::default();
        boxed(&mut s, 20.0);
        let undo = s.undo.len();
        let v = s.execute("inspect.continuity", &json!({})).unwrap();
        assert_eq!(edges(&v).len(), 12);
        for e in edges(&v) {
            let gap = e["gap_mm"]["value"].as_f64().unwrap();
            let angle = e["normal_angle_deg"]["value"].as_f64().unwrap();
            let jump = e["curvature_jump_per_mm"]["value"].as_f64().unwrap();
            assert!(gap < 1e-6, "gap {gap} in {e}");
            assert!((angle - 90.0).abs() < 1e-9, "angle {angle} in {e}");
            assert!(jump < 1e-9, "curvature jump {jump} in {e}");
            assert_eq!(e["verdict"], json!("sharp"), "{e}");
            assert_eq!(e["order"], json!("g0"), "{e}");
            assert_eq!(e["samples"], json!(33), "{e}");
            assert!((e["length_mm"].as_f64().unwrap() - 20.0).abs() < 1e-9, "{e}");
            assert_eq!(e["normal_angle_deg"]["at"].as_array().map(Vec::len), Some(3), "{e}");
            let f = e["normal_angle_deg"]["edge_fraction"].as_f64().unwrap();
            assert!((0.0..=1.0).contains(&f), "{e}");
        }
        assert_eq!(v["analysis"], json!("continuity"));
        assert_eq!(v["samples"], json!(33));
        assert_eq!(
            v["tolerances"],
            json!({"gap_mm": 1e-3, "angle_deg": 1.0, "curvature_per_mm": 0.01, "curvature_change_per_mm2": 1e-3, "curvature_change_rate_per_mm3": 1e-4})
        );
        assert_eq!(v["summary"]["orders"], json!({"g0": 12, "g1": 0, "g2": 0, "g3": 0, "g4": 0}));
        assert_eq!(v["summary"]["edges"], json!(12));
        assert_eq!(v["summary"]["sharp"], json!(12));
        assert_eq!(v["summary"]["worst"], json!("sharp"));
        assert_eq!(s.continuity.len(), 12, "the seams are kept for the viewport");
        assert_eq!(s.undo.len(), undo, "no undo step");
        assert_eq!(s.analysis, None, "only the Continuity analysis is active");
    }

    /// A rolling-ball fillet is tangent to the faces it joins but not curvature continuous: its
    /// two seams read ~0° with the fillet's 1 / radius curvature jump against the planes.
    #[test]
    fn continuity_finds_the_fillet_seams_tangent_and_their_curvature_jump() {
        let mut s = Session::default();
        boxed(&mut s, 20.0);
        s.execute("solid.fillet", &json!({"edges": [[20, 20, 10]], "radius": 4})).unwrap();
        let v = s.execute("inspect.continuity", &json!({})).unwrap();
        assert_eq!(edges(&v).len(), 15, "the rounded edge leaves three new edges behind: {v}");
        let seams = tangent(edges(&v));
        assert_eq!(seams.len(), 2, "the fillet meets its two neighbours tangentially: {v}");
        for e in seams {
            assert!(e["gap_mm"]["value"].as_f64().unwrap() < 1e-6, "{e}");
            let jump = e["curvature_jump_per_mm"]["value"].as_f64().unwrap();
            assert!((jump - 0.25).abs() < 1e-6, "1 / radius 4: {e}");
            assert_eq!(e["verdict"], json!("curvature_jump"), "{e}");
            assert_eq!(e["order"], json!("g1"), "a plane tangent to a cylinder is G1, not G2: {e}");
            assert!(e["curvature_change_jump_per_mm2"]["value"].as_f64().unwrap() < 1e-9, "{e}");
            assert!(e["curvature_change_rate_jump_per_mm3"]["value"].as_f64().unwrap() < 1e-9, "{e}");
        }
        assert_eq!(v["summary"]["orders"]["g1"], json!(2));
        assert_eq!(s.continuity.iter().filter(|seam| seam.order == Some(crate::kernel::Continuity::G1)).count(), 2);
        assert_eq!(v["summary"]["curvature_jump"], json!(2));
        assert_eq!(v["summary"]["sharp"], json!(13));
        assert_eq!(v["summary"]["smooth"], json!(0));
    }

    /// A slot's straight wall runs tangentially into its arc wall: the join is G1 but its
    /// curvature jumps by the arc's 1 / radius, which is exactly what a Class-A check looks for.
    #[test]
    fn continuity_reports_one_over_the_radius_where_a_line_meets_an_arc() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.slot.center_point", &json!({"center": [0, 0], "end": [20, 0], "width": 10})).unwrap();
        s.execute("solid.extrude", &json!({"distance": 5})).unwrap();
        let v = s.execute("inspect.continuity", &json!({"angle_tolerance_deg": 0.01})).unwrap();
        let seams = tangent(edges(&v));
        assert_eq!(seams.len(), 4, "a slot has four line-to-arc joins: {v}");
        for e in seams {
            assert!(e["gap_mm"]["value"].as_f64().unwrap() < 1e-6, "{e}");
            let jump = e["curvature_jump_per_mm"]["value"].as_f64().unwrap();
            assert!((jump - 0.2).abs() < 1e-6, "1 / radius 5: {e}");
            assert_eq!(e["verdict"], json!("curvature_jump"), "{e}");
        }
        assert_eq!(v["summary"]["curvature_jump"], json!(4));
    }

    /// `clear` takes the shading away, and every continuity call stays view state.
    #[test]
    fn continuity_is_view_state_and_clears() {
        let mut s = Session::default();
        boxed(&mut s, 10.0);
        let undo = s.undo.len();
        s.execute("inspect.continuity", &json!({"samples": 5})).unwrap();
        assert_eq!(s.continuity.len(), 12);
        assert_eq!(s.undo.len(), undo, "no undo step");
        let cleared = s.execute("inspect.continuity", &json!({"clear": true})).unwrap();
        assert_eq!(cleared["cleared"], json!(true));
        assert!(s.continuity.is_empty(), "clear leaves nothing shaded");
        assert_eq!(s.undo.len(), undo);
    }

    /// `add` keeps what is shaded and adds the picked edges: an edge measured again replaces its
    /// seam instead of drawing it twice, and a call without `add` starts over.
    #[test]
    fn continuity_add_keeps_the_seams_shown() {
        let mut s = Session::default();
        boxed(&mut s, 10.0);
        s.execute("inspect.continuity", &json!({"edges": [[5, 0, 0]]})).unwrap();
        assert_eq!(s.continuity.len(), 1);
        let v = s.execute("inspect.continuity", &json!({"edges": [[0, 5, 0]], "add": true})).unwrap();
        assert_eq!(edges(&v).len(), 1, "the reply covers the edges this call measured: {v}");
        assert_eq!(s.continuity.len(), 2, "the first seam stays");
        s.execute("inspect.continuity", &json!({"edges": [[5, 0, 0]], "add": true, "samples": 5})).unwrap();
        assert_eq!(s.continuity.len(), 2, "measured again, not added twice");
        assert!(s.continuity.iter().any(|seam| seam.points.len() == 5), "with its new reading");
        assert!(s.continuity.iter().all(|seam| seam.body == "Body1"), "{:?}", s.continuity);
        s.execute("inspect.continuity", &json!({"edges": [[10, 5, 0]]})).unwrap();
        assert_eq!(s.continuity.len(), 1, "without `add` the shading starts over");
        s.execute("inspect.continuity", &json!({"add": true})).unwrap();
        assert_eq!(s.continuity.len(), 12, "every edge once");
    }

    /// Hostile parameters are refused with an error, never a panic.
    #[test]
    fn continuity_details_keep_successful_per_record_settings_and_query_is_pure() {
        let mut s = Session::default();
        boxed(&mut s, 10.0);
        let first = s
            .execute(
                "inspect.continuity",
                &json!({"edges":[[5,0,0]],"samples":5,"gap_tolerance_mm":0.002,"angle_tolerance_deg":2,"curvature_tolerance_per_mm":0.03}),
            )
            .unwrap();
        let old = s.continuity[0].clone();
        s.execute(
            "inspect.continuity",
            &json!({"edges":[[0,5,0]],"samples":9,"gap_tolerance_mm":0.007,"angle_tolerance_deg":4,"curvature_tolerance_per_mm":0.06,"add":true}),
        )
        .unwrap();
        let rows = s.continuity_details();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].samples, 5);
        assert_eq!(rows[0].gap_tolerance_mm, 0.002);
        assert_eq!(rows[1].samples, 9);
        assert_eq!(rows[1].angle_tolerance_deg, 4.0);
        assert!(rows.iter().all(|row| row.current));
        assert_eq!(rows[0].gap_mm, first["bodies"][0]["edges"][0]["gap_mm"]["value"].as_f64().unwrap());
        assert_eq!(rows[0].gap_at, serde_json::from_value::<[f64; 3]>(first["bodies"][0]["edges"][0]["gap_mm"]["at"].clone()).unwrap());
        let before = (s.revision, s.doc.to_json(), s.undo.len(), s.status_activation.clone(), s.continuity.clone(), s.is_dirty());
        for _ in 0..3 {
            let query = s.execute("view.status_details", &json!({})).unwrap();
            assert_eq!(query["continuity"][0]["samples"], 5);
            assert_eq!(query["continuity"][1]["samples"], 9);
        }
        assert_eq!((s.revision, s.doc.to_json(), s.undo.len(), s.status_activation.clone(), s.continuity.clone(), s.is_dirty()), before);
        assert!(s.execute("view.status_details", &json!({"extra":true})).is_err());
        assert!(s.execute("inspect.continuity", &json!({"body":old.body,"edges":[{"index":old.edge}],"samples":2,"add":true})).is_err());
        assert_eq!(s.continuity, before.4, "failed live update is atomic across mixed settings");
        s.execute("inspect.continuity",&json!({"body":old.body,"edges":[{"index":old.edge}],"samples":7,"gap_tolerance_mm":old.details.gap_tolerance_mm,"angle_tolerance_deg":old.details.angle_tolerance_deg,"curvature_tolerance_per_mm":old.details.curvature_tolerance_per_mm,"add":true})).unwrap();
        let remaining = s.continuity.iter().find(|row| row.edge != old.edge).unwrap();
        assert_eq!(remaining.details, rows[1]);
        assert_eq!(s.continuity.iter().find(|row| row.edge == old.edge).unwrap().details.samples, 7);
    }

    #[test]
    fn continuity_targets_follow_geometry_identity_rather_than_view_revision() {
        let mut s = Session::default();
        boxed(&mut s, 10.0);
        s.execute("inspect.continuity", &json!({"edges":[[5,0,0]],"samples":5})).unwrap();
        let seam = s.continuity[0].clone();
        let revision = s.revision;
        s.execute("view.apply", &json!({"camera":{"target":[1,2,3],"yaw":0,"pitch":0,"distance":40}})).unwrap();
        assert_eq!(s.camera.unwrap().target, solidcraft_geom::Vec3::new(1.0, 2.0, 3.0));
        // Camera apply is an echo; these real display changes advance Session's view revision.
        s.execute("browser.visibility", &json!({"items":[seam.body],"visible":false})).unwrap();
        s.execute("browser.visibility", &json!({"items":[seam.body],"visible":true})).unwrap();
        assert!(s.revision > revision);
        assert!(s.continuity_target_points(&seam.body, seam.edge).is_some());
        s.execute("body.rename", &json!({"body":seam.body,"name":"Renamed"})).unwrap();
        assert!(!s.continuity_details()[0].current);
        assert!(s.continuity_target_points(&seam.body, seam.edge).is_none());
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.continuity_details()[0].current, "restored matching geometry may be targeted again");
        let feature = s.doc.features[0].id;
        s.execute("timeline.redefine", &json!({"feature":feature,"command":"solid.box","params":{"length":20,"width":10,"height":10}})).unwrap();
        assert!(!s.continuity_details()[0].current);
        assert!(s.continuity_target_points(&seam.body, seam.edge).is_none());
        s.execute("edit.undo", &json!({})).unwrap();
        let restored_key = s.model.state().body(&seam.body).unwrap().geometry_key();
        assert_ne!(
            restored_key, seam.details.geometry_key,
            "feature redefinition undo produces a new evaluated generation, despite restoring dimensions"
        );
        assert!(!s.continuity_details()[0].current);
        assert!(s.continuity_target_points(&seam.body, seam.edge).is_none());
        s.execute("inspect.continuity",&json!({"body":seam.body,"edges":[{"index":seam.edge}],"samples":seam.details.samples,
            "gap_tolerance_mm":seam.details.gap_tolerance_mm,"angle_tolerance_deg":seam.details.angle_tolerance_deg,"curvature_tolerance_per_mm":seam.details.curvature_tolerance_per_mm})).unwrap();
        assert!(s.continuity_details()[0].current, "a real fresh report validates the restored geometry generation");
        assert!(s.continuity_target_points(&seam.body, seam.edge).is_some());
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(!s.continuity_details()[0].current);
        s.execute("inspect.continuity", &json!({"clear":true})).unwrap();
        assert!(s.continuity_details().is_empty());
    }

    #[test]
    fn continuity_details_reject_changed_occurrence_placement_without_query_evaluation() {
        let mut s = Session::default();
        let component = s.execute("component.create", &json!({"name":"Part","grounded":false})).unwrap();
        let occurrence = component["occurrence"].as_u64().unwrap();
        s.execute("solid.box", &json!({"length":10,"width":10,"height":10,"body_name":"PartBody"})).unwrap();
        s.execute("component.activate", &json!({"component":0})).unwrap();
        s.execute("inspect.continuity", &json!({"body":"PartBody","edges":[[5,0,0]],"samples":5})).unwrap();
        let record = s.continuity[0].clone();
        assert!(s.continuity_details()[0].current);
        s.execute("occurrence.move", &json!({"occurrence":occurrence,"translate":[0,20,0],"capture":false})).unwrap();
        assert!(s.pending_moves.contains_key(&occurrence), "this test intentionally exercises a pending, revertible placement");
        let revision = s.revision;
        assert!(!s.continuity_details()[0].current);
        assert_eq!(s.revision, revision, "descriptor does not prepare a stale world or mutate it");
        let _placed = s.world_state().unwrap();
        assert!(!s.continuity_details()[0].current, "a warmed moved occurrence is still different geometry placement");
        assert!(s.continuity_target_points(&record.body, record.edge).is_none());
        s.execute("component.revert_position", &json!({"occurrences":[occurrence]})).unwrap();
        assert!(!s.pending_moves.contains_key(&occurrence));
        let _restored = s.world_state().unwrap();
        assert!(s.continuity_details()[0].current, "matching restored placement is validated by real geometry identity");
    }

    #[test]
    fn hostile_continuity_params_are_refused() {
        let mut s = Session::default();
        boxed(&mut s, 10.0);
        for params in [
            json!({"samples": 0}),
            json!({"samples": 2}),
            json!({"samples": 258}),
            json!({"samples": 3.5}),
            json!({"samples": "33"}),
            json!({"samples": null}),
            json!({"gap_tolerance_mm": -1}),
            json!({"gap_tolerance_mm": 1e12}),
            json!({"angle_tolerance_deg": f64::NAN}),
            json!({"curvature_tolerance_per_mm": "tight"}),
            json!({"curvature_change_tolerance_per_mm2": -0.5}),
            json!({"curvature_change_rate_tolerance_per_mm3": null}),
            json!({"edges": 3}),
            json!({"edges": [[1, 2]]}),
            json!({"edges": [[]]}),
            json!({"edges": [{"index": 99999}]}),
            json!({"edges": [[500, 500, 500]]}),
            json!({"body": "Nope"}),
        ] {
            assert!(s.execute("inspect.continuity", &params).is_err(), "accepted {params}");
        }
        let many: Vec<Value> = (0..513).map(|i| json!([i as f64, 0.0, 0.0])).collect();
        assert!(s.execute("inspect.continuity", &json!({"edges": many})).is_err(), "513 picks");
        assert!(s.continuity.is_empty(), "a refused call shades nothing");
        assert_eq!(s.undo.len(), 1, "only the box is an undo step");
    }
}
