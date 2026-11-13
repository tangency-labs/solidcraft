//! Exact Forms region selection. Capture is read-only; preparation and queries run off-thread.
use super::*;
use solidcraft_engine::doc::forms::SelectionGuides;
use solidcraft_engine::geom::bspline::HPoint;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

mod curve_pieces;
mod native_face;
mod native_guides;
mod occlusion;
mod projected_curve;

const MAX_ELEMENTS: usize = 262_144;
const PREPARATION_WORK: usize = 64_000_000;
const QUERY_WORK: usize = 64_000_000;
const CANCELLED: &str = "Form selection cancelled";

#[cfg(test)]
#[derive(Clone, Debug)]
struct EdgeCost {
    edge: [usize; 2],
    route: &'static str,
    admission: usize,
    after_extraction: usize,
    after_visibility: usize,
}
#[cfg(test)]
impl EdgeCost {
    fn total(&self) -> usize {
        self.admission.saturating_sub(self.after_visibility)
    }
    fn summary(&self) -> String {
        format!(
            "{} {:?}: entry={}, extracted={}, exit={}, extraction_spent={}, visibility_spent={}",
            self.route,
            self.edge,
            self.admission,
            self.after_extraction,
            self.after_visibility,
            self.admission.saturating_sub(self.after_extraction),
            self.after_extraction.saturating_sub(self.after_visibility),
        )
    }
}

pub(super) struct Query {
    exact: Arc<live_cad::ExactSelection>,
    allowed: Arc<BTreeSet<Element>>,
    projection: Proj,
    region: crate::selection::BoxSel,
    pick: Pick,
    visibility: occlusion::Snapshot,
    stop: Option<Arc<AtomicBool>>,
}

fn spend(remaining: &mut usize, work: usize) -> Result<(), String> {
    *remaining = remaining.checked_sub(work).ok_or("Form selection exceeded its work budget")?;
    Ok(())
}

impl Query {
    /// Freeze exact geometry, logical identity and display-only occlusion together.
    /// A caller must qualify source, camera, filter and selection again before applying the result.
    pub(super) fn capture(app: &SolveApp, projection: &Proj, region: &crate::selection::BoxSel) -> Result<Self, String> {
        let form = app.session.forms_prototype.as_ref().ok_or("no Form is being edited")?;
        if form.repair.is_some() || form.has_tool_preview() {
            return Err("apply or cancel the current Form tool before region selection".into());
        }
        if !projection.rect.is_finite()
            || projection.rect.width() <= 0.0
            || projection.rect.height() <= 0.0
            || projection.vp.0.iter().flatten().any(|value| !value.is_finite())
            || !region.rect.is_finite()
            || region.rect.width() < 0.0
            || region.rect.height() < 0.0
            || region.lasso.len() > 4096
            || (region.is_lasso() && (region.lasso.len() < 3 || region.lasso.iter().any(|point| !point.is_finite())))
        {
            return Err("invalid Form selection region".into());
        }
        let exact = live_cad::selection_geometry(app).ok_or("wait for the current CAD preview before selecting a region")?;
        if exact.faces.len() != exact.body.face_count() || exact.faces.len() > MAX_ELEMENTS {
            return Err("the exact Form preview lost its face ownership".into());
        }
        exact.logical.as_ref().map_err(Clone::clone)?;
        let allowed = live_cad::selection_allowed(app).ok_or("the exact Form preview lost its logical identities")?.clone();
        Ok(Self {
            exact: exact.clone(),
            allowed,
            projection: *projection,
            region: region.clone(),
            pick: app.forms_prototype.pick,
            visibility: occlusion::Snapshot::capture(app, projection)?,
            stop: None,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn run_cancellable(mut self, stop: Arc<AtomicBool>) -> Result<BTreeSet<Element>, String> {
        self.stop = Some(stop);
        self.run()
    }

    fn check_running(&self) -> Result<(), String> {
        if self.stop.as_ref().is_some_and(|stop| stop.load(Ordering::Relaxed)) { Err(CANCELLED.into()) } else { Ok(()) }
    }

    pub(super) fn run(mut self) -> Result<BTreeSet<Element>, String> {
        self.check_running()?;
        let mut preparation_work = PREPARATION_WORK;
        self.visibility.prepare(&mut preparation_work)?;
        self.check_running()?;

        // An All selection has the same per-kind admission as the standalone
        // filters. Each kind is bounded, and the whole result is still atomic.
        let mut selected = BTreeSet::new();
        if matches!(self.pick, Pick::All | Pick::Vertex) {
            let mut work = QUERY_WORK;
            let points = self.exact.points.as_ref().map_err(Clone::clone)?;
            spend(&mut work, self.allowed.len())?;
            for element in self.allowed.iter() {
                self.check_running()?;
                let Element::Vertex(id) = element else { continue };
                spend(&mut work, 1 + self.region.lasso.len())?;
                let point = *points.get(id).ok_or("the exact Form preview lost a logical point")?;
                if self.projection.to_screen(point).is_some_and(|p| self.region.takes(&[p])) && self.visibility.visible(point, &mut work)? {
                    selected.insert(*element);
                }
            }
        }
        if matches!(self.pick, Pick::All | Pick::Edge) {
            let mut work = QUERY_WORK;
            self.edges(&mut work, &mut selected)?;
        }
        if matches!(self.pick, Pick::All | Pick::Face) {
            let mut work = QUERY_WORK;
            self.faces(&mut work, &mut selected)?;
        }
        self.check_running()?;
        Ok(selected)
    }

    fn visible(&self, point: Vec3, work: &mut usize) -> Result<bool, String> {
        self.check_running()?;
        self.visibility.visible(point, work)
    }

    fn occludes_hull(&self, controls: &[HPoint], work: &mut usize) -> Result<bool, String> {
        self.check_running()?;
        self.visibility.occludes_hull(controls, work)
    }

    fn edges(&self, work: &mut usize, selected: &mut BTreeSet<Element>) -> Result<(), String> {
        #[cfg(test)]
        let stage_entry = *work;
        #[cfg(test)]
        let mut costs = Vec::new();
        spend(work, self.allowed.len())?;
        let mut covered = BTreeSet::new();
        match &self.exact.guides {
            SelectionGuides::Subdivision(curves) => {
                for curve in curves {
                    self.check_running()?;
                    spend(work, 1)?;
                    let edge = Element::Edge([curve.edge[0].min(curve.edge[1]), curve.edge[0].max(curve.edge[1])]);
                    if !self.allowed.contains(&edge) {
                        continue;
                    }
                    #[cfg(test)]
                    let admission = *work;
                    if !covered.insert(edge) {
                        return Err("the exact Form preview repeats a logical edge route".into());
                    }
                    let mut pieces = Vec::new();
                    for (_, rational) in curve.rational_pieces_bounded(work).map_err(|error| error.to_string())? {
                        pieces.extend(curve_pieces::bezier_pieces(&rational, work).map_err(str::to_owned)?.into_iter().map(|piece| piece.controls));
                    }
                    #[cfg(test)]
                    let after_extraction = *work;
                    #[cfg(test)]
                    if matches!(curve.edge, [59, 75] | [157, 158]) {
                        self.trace_exact_visibility(curve.edge, &pieces);
                    }
                    let result = self.edge_takes(&pieces, work);
                    #[cfg(test)]
                    self.trace_edge_cost(
                        &mut costs,
                        EdgeCost { edge: curve.edge, route: "subdivision", admission, after_extraction, after_visibility: *work },
                        &pieces,
                        &result,
                    );
                    #[cfg(test)]
                    if result.is_err() {
                        self.trace_failed_certificates(curve.edge, &pieces);
                    }
                    if result.map_err(|error| format!("logical Form edge {:?}: {error}", curve.edge))? {
                        selected.insert(edge);
                    }
                }
            }
            SelectionGuides::Native(native) => {
                for guide in &native.edges {
                    self.check_running()?;
                    spend(work, 1)?;
                    let edge = Element::Edge([guide.edge[0].min(guide.edge[1]), guide.edge[0].max(guide.edge[1])]);
                    if !self.allowed.contains(&edge) {
                        continue;
                    }
                    #[cfg(test)]
                    let admission = *work;
                    if !covered.insert(edge) {
                        return Err("the exact Form preview repeats a logical edge route".into());
                    }
                    let pieces = native_guides::edge_pieces(native, guide, work)?;
                    #[cfg(test)]
                    let after_extraction = *work;
                    #[cfg(test)]
                    if matches!(guide.edge, [59, 75] | [157, 158]) {
                        self.trace_exact_visibility(guide.edge, &pieces);
                    }
                    let result = self.edge_takes(&pieces, work);
                    #[cfg(test)]
                    self.trace_edge_cost(
                        &mut costs,
                        EdgeCost { edge: guide.edge, route: "native", admission, after_extraction, after_visibility: *work },
                        &pieces,
                        &result,
                    );
                    #[cfg(test)]
                    if result.is_err() {
                        self.trace_failed_certificates(guide.edge, &pieces);
                    }
                    if result.map_err(|error| format!("logical native Form edge {:?}: {error}", guide.edge))? {
                        selected.insert(edge);
                    }
                }
            }
            SelectionGuides::Unmapped(error) => return Err(error.clone()),
        }
        #[cfg(test)]
        eprintln!(
            "region edge stage completed: entry={stage_entry}, exit={}, top8=[{}]",
            *work,
            costs.iter().map(EdgeCost::summary).collect::<Vec<_>>().join("; ")
        );
        if self.allowed.iter().any(|element| matches!(element, Element::Edge(_)) && !covered.contains(element)) {
            return Err("the exact Form preview lost a logical edge".into());
        }
        Ok(())
    }

    fn edge_takes(&self, pieces: &[Vec<HPoint>], work: &mut usize) -> Result<bool, String> {
        if pieces.is_empty() {
            return Err("an exact Form edge has no curve pieces".into());
        }
        if !self.region.crossing || self.region.is_lasso() {
            let mut uncertain = None;
            for piece in pieces {
                self.check_running()?;
                let result = projected_curve::project_controls(&self.projection, piece).map_err(str::to_owned).and_then(|projected| {
                    projected.map_or(Ok(false), |controls| projected_curve::curve_takes(&self.region, &controls, work).map_err(str::to_owned))
                });
                match result {
                    Ok(false) => return Ok(false),
                    Ok(true) => {}
                    Err(error) => uncertain = Some(error),
                }
            }
            if let Some(error) = uncertain {
                return Err(error);
            }
        }
        projected_curve::curves_have_visible(
            &self.projection,
            &self.region,
            pieces.iter().map(Vec::as_slice),
            work,
            &mut |point, budget| self.visible(point, budget),
            &mut |controls, budget| {
                self.check_running()?;
                self.visibility.occludes_curve_hull(controls, budget)
            },
        )
    }

    #[cfg(test)]
    fn trace_edge_cost(&self, costs: &mut Vec<EdgeCost>, cost: EdgeCost, pieces: &[Vec<HPoint>], result: &Result<bool, String>) {
        costs.push(cost.clone());
        costs.sort_by_key(|cost| std::cmp::Reverse(cost.total()));
        costs.truncate(8);
        if result.is_ok() {
            return;
        }
        eprintln!(
            "region edge failed budget: {}; result={result:?}; top8=[{}]",
            cost.summary(),
            costs.iter().map(EdgeCost::summary).collect::<Vec<_>>().join("; ")
        );
        eprintln!("region actual edge own-ray memo: {}", self.visibility.own_ray_cache_diagnostic_for_test());
        // One diagnostic replay of the SAME complete restrictions, camera,
        // region and callbacks; it does not change the live query's budget or
        // answer. Root-only certificates cannot distinguish cumulative cost.
        if let Err(error) = self.visibility.clear_own_ray_cache_for_test() {
            eprintln!("region cold edge replay unavailable: {error}");
            return;
        }
        let mut fresh = QUERY_WORK;
        let replay = self.edge_takes(pieces, &mut fresh);
        eprintln!(
            "region fresh full-edge replay: edge={:?}, route={}, original_visibility_entry={}, result={replay:?}, used={}, remaining={fresh}",
            cost.edge,
            cost.route,
            cost.after_extraction,
            QUERY_WORK - fresh,
        );
        eprintln!("region cold edge replay own-ray memo: {}", self.visibility.own_ray_cache_diagnostic_for_test());
    }

    #[cfg(test)]
    fn trace_failed_certificates(&self, edge: [usize; 2], pieces: &[Vec<HPoint>]) {
        self.visibility.without_own_ray_cache_for_test(|| self.trace_failed_certificates_uncached(edge, pieces));
    }

    #[cfg(test)]
    fn trace_failed_certificates_uncached(&self, edge: [usize; 2], pieces: &[Vec<HPoint>]) {
        for (piece, controls) in pieces.iter().take(4).enumerate() {
            eprintln!("region visibility diagnostic edge={edge:?}, piece={piece}: {}", self.visibility.certificate_diagnostic(controls));
            if matches!(edge, [59, 75] | [157, 158] | [155, 158]) {
                for depth in [4_i32, 12, 20, 24] {
                    let width = 0.5_f64.powi(depth);
                    for start in [0.0, 0.5, 1.0 - width] {
                        let restriction = solidcraft_engine::geom::bspline::bezier_segment(controls, start, start + width);
                        let detail = restriction.map_or_else(|error| error.to_string(), |controls| self.visibility.certificate_diagnostic(&controls));
                        eprintln!("region deep certificate diagnostic edge={edge:?}, piece={piece}, depth={depth}, start={start:.10}: {detail}");
                    }
                }
            }
        }
    }

    #[cfg(test)]
    fn trace_exact_visibility(&self, edge: [usize; 2], pieces: &[Vec<HPoint>]) {
        self.visibility.without_own_ray_cache_for_test(|| self.trace_exact_visibility_uncached(edge, pieces));
    }

    #[cfg(test)]
    fn trace_exact_visibility_uncached(&self, edge: [usize; 2], pieces: &[Vec<HPoint>]) {
        let mut probe_work = QUERY_WORK;
        for (piece, controls) in pieces.iter().take(4).enumerate() {
            let mut probes = Vec::new();
            for station in 0..=8 {
                let t = f64::from(station) / 8.0;
                let mut row = controls.clone();
                while row.len() > 1 {
                    row = row.windows(2).map(|pair| pair[0] * (1.0 - t) + pair[1] * t).collect();
                }
                let point = row.first().and_then(|point| point.project());
                let inside = point.and_then(|p| self.projection.to_screen(p)).is_some_and(|p| self.region.takes(&[p]));
                let visible = point.map(|point| self.visibility.visible(point, &mut probe_work));
                probes.push(format!("{station}/8:inside={inside},visible={visible:?}"));
            }
            eprintln!(
                "region exact witness diagnostic edge={edge:?}, piece={piece}, {}: {}",
                self.visibility.certificate_diagnostic(controls),
                probes.join("; ")
            );
        }
    }

    fn faces(&self, work: &mut usize, selected: &mut BTreeSet<Element>) -> Result<(), String> {
        spend(work, self.allowed.len())?;
        let mut groups = BTreeMap::<Element, Vec<usize>>::new();
        for (face, owner) in self.exact.faces.iter().enumerate() {
            self.check_running()?;
            spend(work, 1)?;
            let element = match owner {
                live_cad::ExactFaceOwner::Face(id) => Element::Face(*id),
                live_cad::ExactFaceOwner::Surface => Element::Surface,
                live_cad::ExactFaceOwner::Unmapped => return Err("an exact Form face has no logical owner".into()),
            };
            if !self.allowed.contains(&element) {
                return Err("an exact Form face names an inactive logical owner".into());
            }
            groups.entry(element).or_default().push(face);
        }
        if self.allowed.iter().any(|element| matches!(element, Element::Face(_) | Element::Surface) && !groups.contains_key(element)) {
            return Err("the exact Form preview lost a logical face".into());
        }
        for (element, native) in groups {
            if self.face_takes(&native, work)? {
                selected.insert(element);
            }
        }
        Ok(())
    }

    fn face_takes(&self, native: &[usize], work: &mut usize) -> Result<bool, String> {
        #[cfg(test)]
        let admission = *work;
        let window = !self.region.crossing || self.region.is_lasso();
        let mut prepared = Vec::new();
        let mut uncertain = None;
        for &native_face in native {
            self.check_running()?;
            match native_face::PreparedFace::prepare(&self.exact.body, native_face, work) {
                Ok(face) => {
                    if face.native_face != native_face {
                        return Err("the exact Form preview changed native face identity".into());
                    }
                    if window {
                        match face.takes(&self.projection, &self.region, work) {
                            Ok(false) => return Ok(false),
                            Ok(true) => {}
                            Err(error) => uncertain = Some(error),
                        }
                    }
                    prepared.push(face);
                }
                Err(error) => uncertain = Some(format!("native Form face {native_face}: {error}")),
            }
        }
        #[cfg(test)]
        let after_preparation = *work;
        if window && let Some(error) = uncertain.take() {
            #[cfg(test)]
            eprintln!(
                "region face preparation/containment failed: native_count={}, first8={:?}, entry={admission}, exit={}, result={error}; no qualified complete group replay",
                native.len(),
                native.iter().take(8).collect::<Vec<_>>(),
                *work,
            );
            return Err(error);
        }
        if prepared.is_empty() {
            #[cfg(test)]
            eprintln!(
                "region face has no prepared group: native_count={}, first8={:?}, entry={admission}, exit={}, error={uncertain:?}",
                native.len(),
                native.iter().take(8).collect::<Vec<_>>(),
                *work,
            );
            return uncertain.map_or(Ok(false), Err);
        }
        let visibility = native_face::PreparedFace::group_has_visible(
            &prepared,
            &self.projection,
            &self.region,
            work,
            &mut |point, budget| self.visible(point, budget),
            &mut |controls, budget| self.occludes_hull(controls, budget),
        );
        #[cfg(test)]
        if visibility.is_err() {
            eprintln!(
                "region face failed budget: native_count={}, first8={:?}, prepared_count={}, entry={admission}, after_preparation={after_preparation}, exit={}, preparation_spent={}, visibility_spent={}, result={visibility:?}",
                native.len(),
                native.iter().take(8).collect::<Vec<_>>(),
                prepared.len(),
                *work,
                admission.saturating_sub(after_preparation),
                after_preparation.saturating_sub(*work),
            );
            eprintln!("region actual face own-ray memo: {}", self.visibility.own_ray_cache_diagnostic_for_test());
            if let Err(error) = self.visibility.clear_own_ray_cache_for_test() {
                eprintln!("region cold face replay unavailable: {error}");
                return visibility;
            }
            let mut fresh = QUERY_WORK;
            let replay = native_face::PreparedFace::group_has_visible(
                &prepared,
                &self.projection,
                &self.region,
                &mut fresh,
                &mut |point, budget| self.visibility.visible(point, budget),
                &mut |controls, budget| self.visibility.occludes_hull(controls, budget),
            );
            eprintln!(
                "region fresh full-face-group replay: native_count={}, first8={:?}, prepared_count={}, original_visibility_entry={after_preparation}, prior_preparation_uncertainty={uncertain:?}, result={replay:?}, used={}, remaining={fresh}",
                native.len(),
                native.iter().take(8).collect::<Vec<_>>(),
                prepared.len(),
                QUERY_WORK - fresh,
            );
            eprintln!("region cold face replay own-ray memo: {}", self.visibility.own_ray_cache_diagnostic_for_test());
            self.visibility.without_own_ray_cache_for_test(|| {
                for face in &prepared {
                    eprintln!(
                        "region face visibility diagnostic: {}",
                        face.visibility_diagnostic(
                            &self.projection,
                            &self.region,
                            QUERY_WORK,
                            &mut |point, budget| self.visibility.visible(point, budget),
                            &mut |controls, budget| self.visibility.occludes_hull(controls, budget),
                        )
                    );
                }
            });
        }
        match visibility {
            Ok(true) => Ok(true),
            Ok(false) => uncertain.map_or(Ok(false), Err),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_only_retains_immutable_worker_geometry_and_queries_survive_source_drop() {
        let (mut app, projection, region) = plane();
        let first = Query::capture(&app, &projection, &region).unwrap();
        let second = Query::capture(&app, &projection, &region).unwrap();
        assert!(Arc::ptr_eq(&first.exact, &second.exact));
        assert!(Arc::ptr_eq(&first.allowed, &second.allowed));
        assert!(Arc::ptr_eq(&first.exact.definition, &second.exact.definition));
        let before = app.session.forms_prototype.as_ref().unwrap().source_revision();
        app.forms_prototype = Default::default();
        assert!(Query::capture(&app, &projection, &region).is_err());
        assert_eq!(first.run().unwrap(), second.run().unwrap());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().source_revision(), before);
        assert!(app.forms_prototype.elements.is_empty());
    }

    fn plane() -> (SolveApp, Proj, crate::selection::BoxSel) {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[8,8,1],"segments":[1,1,1],"levels":0})).unwrap();
        app.cam.pitch = std::f64::consts::FRAC_PI_2;
        app.cam.fov = 0.0;
        app.cam.target = Vec3::ZERO;
        app.cam.distance = 20.0;
        live_cad::install_for_test(&mut app);
        let projection = crate::viewport::projection(&app, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 600.0)));
        let region = crate::selection::BoxSel::from_drag(egui::pos2(5.0, 5.0), egui::pos2(795.0, 595.0));
        (app, projection, region)
    }

    #[test]
    fn exact_region_query_unions_all_filters_without_changing_source_or_history() {
        let (mut app, projection, region) = plane();
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        let history = app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].clone();
        let document = app.session.doc.clone();
        let mut union = BTreeSet::new();
        for (pick, count) in [(Pick::Vertex, 4), (Pick::Edge, 4), (Pick::Face, 1)] {
            app.forms_prototype.pick = pick;
            let selected = Query::capture(&app, &projection, &region).unwrap().run().unwrap();
            assert_eq!(selected.len(), count);
            union.extend(selected);
        }
        app.forms_prototype.pick = Pick::All;
        assert_eq!(Query::capture(&app, &projection, &region).unwrap().run().unwrap(), union);
        assert_eq!(union.len(), 9);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], history);
        assert!(Arc::ptr_eq(&app.session.doc, &document));
        assert!(app.forms_prototype.elements.is_empty());
    }

    #[test]
    fn tight_region_queries_qualify_all_eight_native_primitives_and_filter_union() {
        primitive_region_matrix(false);
    }

    #[test]
    fn exact_region_queries_at_ui_default_tolerance_qualify_all_eight_families_and_the_complete_filter_union() {
        primitive_region_matrix(true);
    }

    fn primitive_region_matrix(ui_default_tolerance: bool) {
        use solidcraft_engine::{doc::forms::Definition, geom::subdivision::primitives::Parameters};
        let mut failures = Vec::new();
        for kind in ["box", "plane", "quadball", "sphere", "cylinder", "torus", "tube", "pipe"] {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let parameters =
                    Parameters { kind: kind.into(), dimensions: [10.0; 3], segments: [4, 4, 2], radii: [5.0, 2.0], ..Default::default() };
                let definition = Definition {
                    cage: parameters.build().unwrap(),
                    parameters,
                    placement: Default::default(),
                    imported: None,
                    pipe: None,
                    profile: None,
                    matching: None,
                    symmetry: None,
                    hierarchy: None,
                };
                let mut app = SolveApp::new(Default::default(), Default::default());
                app.session.forms_prototype = Some(solidcraft_engine::forms_prototype::Form::from_definition(definition, 1, None).unwrap());
                if ui_default_tolerance {
                    super::super::opened(&mut app);
                    assert_eq!(super::super::controls::tolerance(&app), Some(app.preferences.forms_cad_tolerance));
                } else {
                    assert_eq!(super::super::controls::tolerance(&app), Some(0.01), "preserve the original tight matrix tolerance");
                }
                let source_before_fit = app.session.forms_prototype.as_ref().unwrap().definition();
                let history_before_fit = app.session.forms_prototype.as_ref().unwrap().inspect();
                let document_before_fit = app.session.doc.clone();
                let camera_before_fit = app.cam;
                live_cad::try_install_for_test(&mut app).unwrap_or_else(|error| panic!("{kind} CAD fit: {error}"));
                assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source_before_fit);
                assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), history_before_fit);
                assert!(Arc::ptr_eq(&app.session.doc, &document_before_fit));
                assert_eq!(app.cam, camera_before_fit);
                assert!(live_cad::selection_geometry(&app).is_some());
                assert!(app.forms_prototype.elements.is_empty());
                super::super::fit(&mut app);
                let projection = crate::viewport::projection(&app, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 600.0)));
                let region = crate::selection::BoxSel::from_drag(egui::pos2(1.0, 1.0), egui::pos2(799.0, 599.0));
                let before = app.session.forms_prototype.as_ref().unwrap().definition();
                let history = app.session.forms_prototype.as_ref().unwrap().inspect();
                let document = app.session.doc.clone();
                let camera = app.cam;
                let mut union = BTreeSet::new();
                for (pick, label) in [(Pick::Vertex, "points"), (Pick::Edge, "edges"), (Pick::Face, "faces")] {
                    app.forms_prototype.pick = pick;
                    let query = Query::capture(&app, &projection, &region).unwrap_or_else(|error| panic!("{kind} capture {label}: {error}"));
                    let allowed = query.allowed.clone();
                    let selected = query.run().unwrap_or_else(|error| panic!("{kind} query {label}: {error}"));
                    assert!(!selected.is_empty(), "{kind} {label}: the framed primitive must have visible elements");
                    if kind == "box" && label == "edges" {
                        assert!(
                            !selected.contains(&Element::Edge([3, 11])),
                            "the rear exact edge must be certified hidden without exhausting the query budget"
                        );
                    }
                    assert!(selected.iter().all(|element| allowed.contains(element)), "{kind}: every returned ID must be an active logical element");
                    assert!(
                        selected.iter().all(|element| matches!(
                            (pick, element),
                            (Pick::Vertex, Element::Vertex(_)) | (Pick::Edge, Element::Edge(_)) | (Pick::Face, Element::Face(_) | Element::Surface)
                        )),
                        "{kind}: the requested filter must be respected"
                    );
                    union.extend(selected);
                }
                app.forms_prototype.pick = Pick::All;
                let selected = Query::capture(&app, &projection, &region).unwrap().run().unwrap_or_else(|error| panic!("{kind} All query: {error}"));
                assert_eq!(selected, union, "{kind}: All must contain the complete point, edge and face union");
                assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
                assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), history);
                assert!(Arc::ptr_eq(&app.session.doc, &document));
                assert_eq!(app.cam, camera);
                assert!(app.forms_prototype.elements.is_empty());
            }));
            if let Err(payload) = result {
                let message = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|message| (*message).to_owned()))
                    .unwrap_or_else(|| "nontext primitive qualification panic".to_owned());
                failures.push(format!("{kind}: {message}"));
            }
        }
        assert!(
            failures.is_empty(),
            "Forms primitive region qualification failures (ui_default_tolerance={ui_default_tolerance}):\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn exact_region_query_refuses_stale_preview_and_hostile_shapes_atomically() {
        let (mut app, projection, mut region) = plane();
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        region.lasso = vec![egui::pos2(1.0, 1.0), egui::pos2(9.0, 1.0), egui::pos2(f32::NAN, 9.0)];
        assert!(Query::capture(&app, &projection, &region).is_err());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        region.lasso.clear();
        let point = app.session.forms_prototype.as_ref().unwrap().control_positions()[0] + Vec3::Z;
        app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[point.x,point.y,point.z]]})).unwrap();
        let edited = app.session.forms_prototype.as_ref().unwrap().definition();
        assert!(Query::capture(&app, &projection, &region).is_err());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), edited);
        assert!(app.forms_prototype.elements.is_empty());
    }

    #[test]
    fn exact_region_query_preserves_refinement_and_uses_active_logical_ids() {
        let (mut app, projection, region) = plane();
        app.session.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        live_cad::install_for_test(&mut app);
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        let query = Query::capture(&app, &projection, &region).unwrap();
        let allowed = query.allowed.clone();
        let found = query.run().unwrap();
        assert!(!found.is_empty());
        assert!(found.iter().all(|element| allowed.contains(element)));
        assert!(found.iter().any(|element| matches!(element, Element::Face(_))));
        assert!(before.hierarchy.is_some());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert!(app.session.doc.features.is_empty());
    }

    #[test]
    fn requested_profile_cells_falling_back_to_one_skin_remain_face_selectable_without_guides() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("solid.cylinder", &json!({"radius":3,"height":2,"body_name":"Support"})).unwrap();
        let body = app.session.model.state().bodies[0].body.clone();
        let top = (0..body.face_count()).find(|&face| body.planar_face(face).is_some_and(|plane| plane.normal().z > 0.99)).unwrap();
        app.session.execute("form.surface", &json!({"body":"Support","face":top,"segments":[3,2],"continuity":0,"partition":true})).unwrap();
        app.cam.pitch = std::f64::consts::FRAC_PI_2;
        app.cam.fov = 0.0;
        app.cam.target = Vec3::new(0.0, 0.0, 2.0);
        app.cam.distance = 20.0;
        live_cad::install_for_test(&mut app);
        let exact = live_cad::selection_geometry(&app).unwrap();
        assert!(exact.definition.profile.as_ref().unwrap().partition, "the authored request must stay intact");
        assert!(exact.faces.iter().all(|owner| *owner == live_cad::ExactFaceOwner::Surface));
        assert!(!exact.logical.as_ref().unwrap().active.contains(&Element::Face(0)));
        app.forms_prototype.elements.insert(Element::Face(0));
        super::super::selection::sync(&mut app);
        assert!(app.forms_prototype.elements.is_empty(), "a whole-skin fit must not retain an unavailable cell preselection");
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        let projection = crate::viewport::projection(&app, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 600.0)));
        let region = crate::selection::BoxSel::from_drag(egui::pos2(5.0, 5.0), egui::pos2(795.0, 595.0));
        app.forms_prototype.pick = Pick::Face;
        let mut query = Query::capture(&app, &projection, &region).unwrap();
        assert!(query.allowed.contains(&Element::Surface));
        assert!(!query.allowed.iter().any(|element| matches!(element, Element::Face(_))));
        let logical = query.exact.logical.as_ref().unwrap();
        assert!(logical.accepts(&BTreeSet::from([Element::Surface]), Pick::Face));
        assert!(!logical.accepts(&BTreeSet::from([Element::Face(0)]), Pick::Face));
        Arc::make_mut(&mut query.exact).guides = SelectionGuides::Unmapped("test unavailable guide contract".into());
        Arc::make_mut(&mut query.exact).points = Err("test unavailable point contract".into());
        assert_eq!(query.run().unwrap(), BTreeSet::from([Element::Surface]));
        app.forms_prototype.pick = Pick::All;
        let mut query = Query::capture(&app, &projection, &region).unwrap();
        Arc::make_mut(&mut query.exact).guides = SelectionGuides::Unmapped("test unavailable guide contract".into());
        Arc::make_mut(&mut query.exact).points = Err("test unavailable point contract".into());
        assert!(query.run().is_err(), "All must not quietly omit unavailable point/edge families");
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert!(app.forms_prototype.elements.is_empty());
    }

    #[test]
    fn exact_region_query_refuses_missing_faces_and_incomplete_or_duplicate_edge_routes() {
        let (mut app, projection, region) = plane();
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        app.forms_prototype.pick = Pick::Face;
        let mut query = Query::capture(&app, &projection, &region).unwrap();
        Arc::make_mut(&mut query.exact).faces.clear();
        assert!(query.run().unwrap_err().contains("lost a logical face"));
        app.forms_prototype.pick = Pick::Edge;
        for duplicate in [false, true] {
            let mut query = Query::capture(&app, &projection, &region).unwrap();
            let SelectionGuides::Subdivision(curves) = &mut Arc::make_mut(&mut query.exact).guides else {
                panic!("a base plane has subdivision source routes")
            };
            if duplicate {
                curves.push(curves[0].clone());
            } else {
                curves.pop();
            }
            let error = query.run().unwrap_err();
            assert!(error.contains(if duplicate { "repeats a logical edge route" } else { "lost a logical edge" }), "{error}");
        }
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert!(app.forms_prototype.elements.is_empty());
    }

    #[test]
    fn one_logical_face_windows_require_every_native_piece_and_crossings_need_any_visible_piece() {
        use solidcraft_engine::{doc::kernel, geom::bspline::NurbsSurface};
        let (mut app, projection, _) = plane();
        app.forms_prototype.pick = Pick::Face;
        let sheet = |x: f64| {
            let controls =
                [(x - 1.0, -1.0), (x + 1.0, -1.0), (x - 1.0, 1.0), (x + 1.0, 1.0)].map(|(x, y)| HPoint::weighted(Vec3::new(x, y, 2.0), 1.0)).to_vec();
            kernel::nurbs_sheet(&NurbsSurface::from_homogeneous([1, 1], [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]], controls).unwrap())
                .unwrap()
        };
        let a = sheet(0.0);
        let b = sheet(10.0);
        let body = kernel::stitch(&[&a, &b], 1e-7).unwrap();
        assert_eq!(body.face_count(), 2);
        let a = projection.to_screen(Vec3::new(-2.0, -2.0, 2.0)).unwrap();
        let b = projection.to_screen(Vec3::new(2.0, 2.0, 2.0)).unwrap();
        for crossing in [false, true] {
            let mut region = crate::selection::BoxSel::from_drag(a.min(b), a.max(b));
            region.crossing = crossing;
            let mut query = Query::capture(&app, &projection, &region).unwrap();
            Arc::make_mut(&mut query.exact).body = body.clone();
            Arc::make_mut(&mut query.exact).faces = vec![live_cad::ExactFaceOwner::Face(0); 2];
            let selected = query.run().unwrap();
            assert_eq!(selected.contains(&Element::Face(0)), crossing);
        }
    }
}
