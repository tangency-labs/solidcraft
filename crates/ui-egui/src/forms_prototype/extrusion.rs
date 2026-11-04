//! Coalesced matched extrusion. Pointer input authors the newest absolute target;
//! only a qualified result enters the Form and its existing gesture history.
use super::*;

#[derive(Clone, PartialEq)]
struct Source {
    document: String,
    revision: u64,
    form: Option<solidcraft_engine::recovery::FormRevision>,
    preview: Option<(u64, bool)>,
}
impl Source {
    fn current(app: &SolveApp) -> Self {
        Self {
            document: app.session.document_id.clone(),
            revision: app.session.authoring_revision(),
            form: solidcraft_engine::recovery::form_revision(&app.session),
            preview: app.session.forms_prototype.as_ref().map(|f| (f.revision, f.has_tool_preview())),
        }
    }
}
struct Pending {
    preview: Option<std::sync::mpsc::Receiver<Result<solidcraft_engine::forms_prototype::ExtrusionResponse, String>>>,
    sequence: u64,
    done: std::sync::mpsc::Receiver<Result<solidcraft_engine::ComputedCommands, String>>,
}
struct Gesture {
    response: Option<solidcraft_engine::forms_prototype::ExtrusionResponse>,
    feedback: Option<(u64, solidcraft_engine::forms_prototype::ExtrusionPreview)>,
    feedback_error: Option<String>,
    preview_attempted: bool,
    source: Source,
    parameters: serde_json::Value,
    sequence: u64,
    settled: Option<u64>,
    accepted: bool,
    released: bool,
    pending: Option<Pending>,
}
#[derive(Default)]
pub(super) struct State {
    gesture: Option<Gesture>,
    timing: Timing,
}
#[derive(Default)]
struct Timing {
    prepare_started_s: Option<f64>,
    prepare_ready_s: Option<f64>,
    samples: std::collections::VecDeque<Sample>,
}
struct Sample {
    sequence: u64,
    target: Vec3,
    requested_s: f64,
    feedback_ready_s: Option<f64>,
    scene_submitted_s: Option<f64>,
    accepted_before: bool,
    witness: Option<serde_json::Value>,
}
impl Timing {
    fn requested(&mut self, gesture: &Gesture, now: f64) {
        if self.samples.back().is_some_and(|sample| sample.sequence == gesture.sequence) {
            return;
        }
        if let Ok(target) = displacement(&gesture.parameters) {
            if self.samples.len() >= 256 {
                self.samples.pop_front();
            }
            self.samples.push_back(Sample {
                sequence: gesture.sequence,
                target,
                requested_s: now,
                feedback_ready_s: None,
                scene_submitted_s: None,
                accepted_before: gesture.accepted,
                witness: None,
            });
        }
    }
    fn feedback(&mut self, gesture: &Gesture, now: f64) {
        if gesture.feedback.as_ref().is_some_and(|(sequence, _)| *sequence == gesture.sequence)
            && let Some(sample) = self.samples.back_mut().filter(|sample| sample.sequence == gesture.sequence)
        {
            sample.feedback_ready_s.get_or_insert(now);
        }
    }
    fn inspect(&self) -> serde_json::Value {
        json!({"clock":"egui_frame_start_seconds","scope":"input_delivery_to_scene_submission_not_gpu_presentation",
            "prepare_started_s":self.prepare_started_s,"prepare_ready_s":self.prepare_ready_s,
            "samples":self.samples.iter().map(|sample|json!({"sequence":sample.sequence,
                "target":[sample.target.x,sample.target.y,sample.target.z],"requested_s":sample.requested_s,
                "feedback_ready_s":sample.feedback_ready_s,"scene_submitted_s":sample.scene_submitted_s,
                "accepted_before":sample.accepted_before,"witness":sample.witness})).collect::<Vec<_>>()})
    }
}
pub(super) fn active(app: &SolveApp) -> bool {
    app.forms_prototype.extrusion.gesture.is_some()
}
pub(super) fn released(app: &SolveApp) -> bool {
    app.forms_prototype.extrusion.gesture.as_ref().is_some_and(|g| g.released)
}
pub(super) fn inspect(app: &SolveApp) -> serde_json::Value {
    let mut result = app.forms_prototype.extrusion.gesture.as_ref().map_or_else(
        || json!({"active":false}),
        |g| json!({"active":true,"released":g.released,"pending":g.pending.is_some(),"sequence":g.sequence,"settled":g.settled,"accepted":g.accepted,"ghost_sequence":g.feedback.as_ref().map(|(sequence,_)|*sequence),"ghost_preparing":g.pending.as_ref().is_some_and(|p|p.preview.is_some()),"ghost_error":g.feedback_error}),
    );
    result["timing"] = app.forms_prototype.extrusion.timing.inspect();
    result["timing"]["observed_s"] = json!(app.now);
    result
}
/// Preserve the ordinary mesh edit path. Matched edits only queue pointer input.
pub(super) fn command(app: &mut SolveApp, mut parameters: serde_json::Value) -> bool {
    if app.session.forms_prototype.as_ref().is_none_or(|f| f.matching.is_none()) {
        return run(app, "form.prototype.extrude_drag", parameters);
    }
    parameters["checkpoint"] = json!(false);
    let source = Source::current(app);
    let slot = &mut app.forms_prototype.extrusion.gesture;
    if let Some(gesture) = slot {
        if gesture.source != source || gesture.released {
            stale(app);
            return false;
        }
        if gesture.parameters != parameters {
            gesture.sequence = gesture.sequence.wrapping_add(1);
            gesture.parameters = parameters;
        }
    } else {
        app.forms_prototype.extrusion.timing = Timing::default();
        *slot = Some(Gesture {
            response: None,
            feedback: None,
            feedback_error: None,
            preview_attempted: false,
            source,
            parameters,
            sequence: 0,
            settled: None,
            accepted: false,
            released: false,
            pending: None,
        });
    }
    if let Some(gesture) = &mut app.forms_prototype.extrusion.gesture {
        app.forms_prototype.extrusion.timing.requested(gesture, app.now);
        feedback(gesture, app.preferences.forms_mesh_while_dragging);
        app.forms_prototype.extrusion.timing.feedback(gesture, app.now);
    }
    true
}
/// Keep a released gesture alive until its final displacement is qualified.
pub(super) fn release(app: &mut SolveApp) -> bool {
    let Some(gesture) = &mut app.forms_prototype.extrusion.gesture else { return false };
    gesture.released = true;
    true
}
/// Dropping the receiver invalidates any outstanding result. Undo only an edit
/// this gesture installed, and only while its accepted source remains current.
pub(super) fn cancel(app: &mut SolveApp) -> bool {
    let Some(gesture) = app.forms_prototype.extrusion.gesture.take() else { return false };
    if gesture.accepted && gesture.source == Source::current(app) {
        run(app, "form.prototype.undo", json!({}));
    }
    true
}
fn stale(app: &mut SolveApp) {
    app.forms_prototype.extrusion.gesture = None;
    app.forms_prototype.drag = None;
    app.forms_prototype.gizmo_drag = None;
    app.set_status(app.tr("forms.prototype.changed_during_fit").to_owned(), true);
}
fn displacement(parameters: &serde_json::Value) -> Result<Vec3, String> {
    let args: solidcraft_engine::schema::forms_prototype::ExtrudeDrag = serde_json::from_value(parameters.clone()).map_err(|e| e.to_string())?;
    Ok(Vec3::new(args.displacement[0], args.displacement[1], args.displacement[2]))
}
fn feedback(gesture: &mut Gesture, enabled: bool) {
    if !enabled || gesture.settled == Some(gesture.sequence) {
        gesture.feedback = None;
        return;
    }
    if gesture.feedback.as_ref().is_some_and(|(sequence, _)| *sequence == gesture.sequence) {
        return;
    }
    if let Some(response) = &gesture.response {
        match displacement(&gesture.parameters).and_then(|delta| response.preview(delta).map_err(|e| e.to_string())) {
            Ok(preview) => {
                gesture.feedback = Some((gesture.sequence, preview));
                gesture.feedback_error = None;
            }
            Err(error) => {
                gesture.feedback = None;
                gesture.feedback_error = Some(error);
            }
        }
    }
}
pub(super) fn preview(app: &SolveApp) -> Option<&solidcraft_engine::forms_prototype::ExtrusionPreview> {
    if !app.preferences.forms_mesh_while_dragging {
        return None;
    }
    let gesture = app.forms_prototype.extrusion.gesture.as_ref()?;
    if gesture.source != Source::current(app) || gesture.settled == Some(gesture.sequence) {
        return None;
    }
    gesture.feedback.as_ref().filter(|(sequence, _)| *sequence == gesture.sequence).map(|(_, preview)| preview)
}
pub(super) fn preview_revision(app: &SolveApp) -> Option<(u64, [u64; 3])> {
    preview(app)?;
    let gesture = app.forms_prototype.extrusion.gesture.as_ref()?;
    let delta = displacement(&gesture.parameters).ok()?;
    Some((gesture.sequence, [delta.x.to_bits(), delta.y.to_bits(), delta.z.to_bits()]))
}
pub(super) fn suspend(app: &mut SolveApp) {
    if let Some(gesture) = &mut app.forms_prototype.extrusion.gesture {
        gesture.feedback = None;
        gesture.response = None;
        gesture.preview_attempted = true;
        if let Some(pending) = &mut gesture.pending {
            pending.preview = None;
        }
    }
}

pub(super) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    poll(app);
    dispatch(app, ctx);
}
fn poll(app: &mut SolveApp) {
    let Some(mut gesture) = app.forms_prototype.extrusion.gesture.take() else { return };
    if gesture.source != Source::current(app) {
        stale(app);
        return;
    }
    if let Some(receiver) = gesture.pending.as_mut().and_then(|pending| pending.preview.take()) {
        match receiver.try_recv() {
            Ok(Ok(response)) => {
                gesture.response = Some(response);
                app.forms_prototype.extrusion.timing.prepare_ready_s = Some(app.now);
            }
            Ok(Err(error)) => gesture.feedback_error = Some(error),
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if let Some(pending) = &mut gesture.pending {
                    pending.preview = Some(receiver);
                }
            }
            Err(_) => gesture.feedback_error = Some("Extrusion feedback preparation stopped".into()),
        }
    }
    feedback(&mut gesture, app.preferences.forms_mesh_while_dragging);
    app.forms_prototype.extrusion.timing.feedback(&gesture, app.now);
    let result = gesture.pending.as_ref().and_then(|pending| match pending.done.try_recv() {
        Ok(result) => Some(result),
        Err(std::sync::mpsc::TryRecvError::Empty) => None,
        Err(_) => Some(Err("Form computation stopped".into())),
    });
    if let Some(result) = result
        && let Some(pending) = gesture.pending.take()
        && pending.sequence == gesture.sequence
    {
        gesture.settled = Some(pending.sequence);
        gesture.feedback = None;
        match result.and_then(|prepared| app.session.accept_commands(prepared).map_err(|e| e.to_string())) {
            Ok(replies) => {
                for (id, reply) in replies {
                    let _ = app.command_completed(&id, Ok(reply));
                }
                gesture.accepted = true;
                gesture.source = Source::current(app);
                if let Some(form) = &app.session.forms_prototype {
                    app.forms_prototype.selected = app.forms_prototype.elements.iter().flat_map(|e| e.vertices(&form.logical)).collect();
                }
            }
            Err(error) => app.set_status(error, true),
        }
    }
    if gesture.released && gesture.pending.is_none() && gesture.settled == Some(gesture.sequence) {
        if gesture.accepted {
            select_extruded_edges(app);
        }
    } else {
        app.forms_prototype.extrusion.gesture = Some(gesture);
    }
}
fn dispatch(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(gesture) = &app.forms_prototype.extrusion.gesture else { return };
    if gesture.pending.is_some() || gesture.settled == Some(gesture.sequence) {
        return;
    }
    #[cfg(target_arch = "wasm32")]
    if !gesture.released {
        // Browser computations are synchronous until a worker host is available.
        return;
    }
    let sequence = gesture.sequence;
    let mut parameters = gesture.parameters.clone();
    parameters["checkpoint"] = json!(!gesture.accepted);
    let copy = app.session.scratch();
    let prepare_preview = app.preferences.forms_mesh_while_dragging && !gesture.preview_attempted;
    if prepare_preview {
        app.forms_prototype.extrusion.timing.prepare_started_s = Some(app.now);
    }
    let preview_parameters = parameters.clone();
    let (preview_tx, preview_rx) = std::sync::mpsc::channel();
    let (tx, rx) = std::sync::mpsc::channel();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let wake = ctx.clone();
        if let Err(error) = std::thread::Builder::new().name("form-extrusion".into()).spawn(move || {
            if prepare_preview {
                let preview = (|| {
                    let args: solidcraft_engine::schema::forms_prototype::ExtrudeDrag =
                        serde_json::from_value(preview_parameters).map_err(|e| e.to_string())?;
                    let form = copy.forms_prototype.as_ref().ok_or_else(|| "No Form remains for extrusion feedback".to_owned())?;
                    form.prepare_extrusion_preview(
                        &args.faces,
                        &args.edges,
                        Vec3::new(args.displacement[0], args.displacement[1], args.displacement[2]),
                        args.checkpoint,
                    )
                    .map_err(|e| e.to_string())
                })();
                let _ = preview_tx.send(preview);
                wake.request_repaint();
            }
            let result = copy.compute_commands(&[("form.prototype.extrude_drag".into(), parameters)], None).map_err(|e| e.to_string());
            let _ = tx.send(result);
            wake.request_repaint();
        }) {
            if let Some(gesture) = &mut app.forms_prototype.extrusion.gesture {
                gesture.settled = Some(sequence);
            }
            app.set_status(error.to_string(), true);
            return;
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (prepare_preview, preview_parameters, preview_tx);
        let result = copy.compute_commands(&[("form.prototype.extrude_drag".into(), parameters)], None).map_err(|e| e.to_string());
        let _ = tx.send(result);
        ctx.request_repaint();
    }
    if let Some(gesture) = &mut app.forms_prototype.extrusion.gesture {
        gesture.preview_attempted |= prepare_preview;
        gesture.pending =
            Some(Pending { preview: if prepare_preview && !cfg!(target_arch = "wasm32") { Some(preview_rx) } else { None }, sequence, done: rx });
    }
}

/// Records the actual ghost geometry handed to the viewport painter. This is
/// scene submission, not a GPU completion or monitor-presentation timestamp.
pub(super) fn scene_submitted(app: &mut SolveApp, proj: &Proj) {
    let Some(ghost) = preview(app) else { return };
    let Some(gesture) = &app.forms_prototype.extrusion.gesture else { return };
    let sequence = gesture.sequence;
    if app.forms_prototype.extrusion.timing.samples.back().is_none_or(|sample| sample.sequence != sequence || sample.scene_submitted_s.is_some()) {
        return;
    }
    let face = gesture.parameters.get("faces").and_then(|faces| faces.get(0)).and_then(serde_json::Value::as_u64);
    let triangle =
        face.and_then(|face| ghost.mesh.tri_face.iter().position(|&owner| u64::from(owner) == face)).and_then(|id| ghost.mesh.triangles.get(id));
    let witness = triangle.and_then(|triangle| {
        let [Some(a), Some(b), Some(c)] = triangle.map(|id| ghost.mesh.positions.get(id as usize).copied()) else { return None };
        let points = [a, b, c];
        let screen = points.map(|point| proj.to_screen(point).map(|point| [point.x, point.y]));
        Some(json!({"face":face,"world":points.map(|point|[point.x,point.y,point.z]),"screen":screen,
            "mesh_positions":ghost.mesh.positions.len(),"triangles":ghost.mesh.triangles.len()}))
    });
    if let Some(sample) = app.forms_prototype.extrusion.timing.samples.back_mut().filter(|sample| sample.sequence == sequence) {
        sample.scene_submitted_s = Some(app.now);
        sample.witness = witness;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matched() -> (SolveApp, usize) {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("solid.box", &json!({"length":8,"width":8,"height":1,"corner":[-4,-4,-1],"body_name":"Support"})).unwrap();
        let body = app.session.model.state().bodies[0].body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[8,8,1],"segments":[4,4,1],"levels":1})).unwrap();
        let ring = app.session.forms_prototype.as_ref().unwrap().cage.boundary_loops().unwrap().remove(0);
        app.session.execute("form.prototype.match", &json!({"body":"Support","face":top,"edge":[ring[0],ring[1]],"order":1})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let face = form.logical.faces.iter().position(|f| f.iter().all(|v| !ring.contains(v))).unwrap();
        app.forms_prototype.elements.insert(Element::Face(face));
        (app, face)
    }
    fn request(app: &mut SolveApp, face: usize, z: f64) {
        assert!(command(app, json!({"faces":[face],"edges":[],"displacement":[0,0,z],"checkpoint":false})));
    }
    fn ready(app: &mut SolveApp) {
        let gesture = app.forms_prototype.extrusion.gesture.as_ref().unwrap();
        let mut parameters = gesture.parameters.clone();
        parameters["checkpoint"] = json!(!gesture.accepted);
        let prepared = app.session.scratch().compute_commands(&[("form.prototype.extrude_drag".into(), parameters)], None).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Ok(prepared)).unwrap();
        let gesture = app.forms_prototype.extrusion.gesture.as_mut().unwrap();
        gesture.pending = Some(Pending { preview: None, sequence: gesture.sequence, done: rx });
    }
    #[test]
    fn newest_absolute_displacement_is_qualified_once_and_release_installs_it() {
        for accept_intermediate in [false, true] {
            let (mut app, face) = matched();
            let original = app.session.forms_prototype.as_ref().unwrap().definition();
            let depth = app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
            let mut expected = app.session.scratch();
            expected.execute("form.prototype.topology", &json!({"tool":"extrude_faces","faces":[face],"displacement":[0,0,0.75]})).unwrap();
            request(&mut app, face, 0.2);
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), original, "queueing must not solve on the pointer path");
            ready(&mut app);
            if accept_intermediate {
                poll(&mut app);
                assert!(active(&app));
            }
            request(&mut app, face, 0.75);
            assert!(release(&mut app));
            assert!(released(&app));
            if !accept_intermediate {
                poll(&mut app);
                assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), original, "an obsolete displacement cannot enter history");
                assert!(active(&app));
            }
            ready(&mut app);
            poll(&mut app);
            assert!(!active(&app));
            let form = app.session.forms_prototype.as_ref().unwrap();
            assert_eq!(form.definition(), expected.forms_prototype.as_ref().unwrap().definition());
            assert_eq!(form.inspect()["undo"], depth + 1);
            let after = form.definition();
            app.session.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), original);
            app.session.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), after);
        }
    }
    #[test]
    fn external_document_draft_and_recreated_editor_changes_refuse_pending_results() {
        for change in [0, 1, 2] {
            let (mut app, face) = matched();
            request(&mut app, face, 0.5);
            ready(&mut app);
            match change {
                0 => {
                    app.session.execute("parameters.add", &json!({"name":"other","expression":"2 mm"})).unwrap();
                }
                1 => {
                    app.session.execute("form.prototype.finish_options", &json!({"tolerance":"0.02 mm"})).unwrap();
                }
                _ => {
                    app.session.execute("form.prototype.close", &json!({})).unwrap();
                    app.session
                        .execute("form.prototype.create", &json!({"primitive":"face","dimensions":[8,8,1],"segments":[4,4,1],"levels":1}))
                        .unwrap();
                }
            }
            let document = app.session.doc.clone();
            let draft = app.session.forms_prototype.as_ref().unwrap().definition();
            let depth = app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].clone();
            poll(&mut app);
            assert!(!active(&app));
            assert_eq!(app.session.doc, document);
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), draft);
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], depth);
        }
    }
    #[test]
    fn cancelling_pending_work_undoes_only_accepted_gesture_edits() {
        for accepted in [false, true] {
            let (mut app, face) = matched();
            let original = app.session.forms_prototype.as_ref().unwrap().definition();
            let depth = app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
            request(&mut app, face, 0.2);
            ready(&mut app);
            if accepted {
                poll(&mut app);
                request(&mut app, face, 0.75);
                ready(&mut app);
            }
            assert!(cancel(&mut app));
            poll(&mut app);
            assert!(!active(&app));
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), original);
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], depth);
        }
    }
    #[test]
    fn latest_failure_reports_the_error_and_retains_the_last_qualified_state() {
        for accepted in [false, true] {
            let (mut app, face) = matched();
            request(&mut app, face, 0.2);
            if accepted {
                ready(&mut app);
                poll(&mut app);
            }
            let last_valid = app.session.forms_prototype.as_ref().unwrap().inspect();
            let definition = app.session.forms_prototype.as_ref().unwrap().definition();
            request(&mut app, face, 0.75);
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(Err("Match continuity could not be qualified".into())).unwrap();
            let gesture = app.forms_prototype.extrusion.gesture.as_mut().unwrap();
            gesture.pending = Some(Pending { preview: None, sequence: gesture.sequence, done: rx });
            release(&mut app);
            poll(&mut app);
            assert!(!active(&app));
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), last_valid);
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), definition);
            assert!(app.status.as_ref().is_some_and(|(text, _, error)| *error && text == "Match continuity could not be qualified"));
        }
    }
    #[test]
    fn released_edge_extrusion_selects_its_translated_rim_after_final_apply() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[8,8,1],"segments":[1,1,1],"levels":1})).unwrap();
        let edge = app.session.forms_prototype.as_ref().unwrap().cage.edges().unwrap()[0];
        app.forms_prototype.elements.insert(Element::Edge(edge));
        // The worker's adoption and selection contract is independent of the solver.
        app.forms_prototype.extrusion.gesture = Some(Gesture {
            response: None,
            feedback: None,
            feedback_error: None,
            preview_attempted: false,
            source: Source::current(&app),
            parameters: json!({"faces":[],"edges":[edge],"displacement":[0,0,1]}),
            sequence: 0,
            settled: None,
            accepted: false,
            released: true,
            pending: None,
        });
        ready(&mut app);
        assert_eq!(app.forms_prototype.elements, BTreeSet::from([Element::Edge(edge)]));
        poll(&mut app);
        assert!(!active(&app));
        let translated = app.session.forms_prototype.as_ref().unwrap().extruded_edges();
        assert!(!translated.is_empty());
        assert_eq!(app.forms_prototype.elements, translated.iter().copied().map(Element::Edge).collect());
        assert_eq!(app.forms_prototype.selected, translated.iter().flatten().copied().collect());
        assert!(!app.forms_prototype.elements.contains(&Element::Edge(edge)));
    }
    fn delayed_feedback(app: &mut SolveApp) -> std::sync::mpsc::Sender<Result<solidcraft_engine::ComputedCommands, String>> {
        let gesture = app.forms_prototype.extrusion.gesture.as_ref().unwrap();
        let args: solidcraft_engine::schema::forms_prototype::ExtrudeDrag = serde_json::from_value(gesture.parameters.clone()).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let response = form
            .prepare_extrusion_preview(
                &args.faces,
                &args.edges,
                Vec3::new(args.displacement[0], args.displacement[1], args.displacement[2]),
                !gesture.accepted,
            )
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (preview_tx, preview_rx) = std::sync::mpsc::channel();
        preview_tx.send(Ok(response)).unwrap();
        let gesture = app.forms_prototype.extrusion.gesture.as_mut().unwrap();
        gesture.preview_attempted = true;
        gesture.pending = Some(Pending { preview: Some(preview_rx), sequence: gesture.sequence, done: rx });
        tx
    }
    #[test]
    fn latest_ghost_moves_before_the_first_qualified_result_without_authoring_an_edit() {
        let (mut app, face) = matched();
        let source = app.session.forms_prototype.as_ref().unwrap().inspect();
        let recovery = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
        app.now = 10.0;
        request(&mut app, face, 0.2);
        let _delay = delayed_feedback(&mut app);
        app.now = 10.3;
        poll(&mut app);
        let proj =
            Proj { cam: app.cam, vp: app.cam.view_proj(900.0 / 700.0, 20.0), rect: egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)) };
        scene_submitted(&mut app, &proj);
        let first_timing = inspect(&app)["timing"].clone();
        assert_eq!(first_timing["samples"][0]["requested_s"], 10.0);
        assert_eq!(first_timing["samples"][0]["feedback_ready_s"], 10.3);
        assert_eq!(first_timing["samples"][0]["scene_submitted_s"], 10.3);
        assert!(first_timing["samples"][0]["witness"]["triangles"].as_u64().unwrap() > 0);
        let first = preview(&app).unwrap().mesh.positions.clone();
        app.now = 10.4;
        request(&mut app, face, 0.8);
        app.now = 10.45;
        scene_submitted(&mut app, &proj);
        let timing = inspect(&app)["timing"].clone();
        assert_eq!(timing["samples"][1]["target"], json!([0.0, 0.0, 0.8]));
        assert_eq!(timing["samples"][1]["requested_s"], 10.4);
        assert_eq!(timing["samples"][1]["feedback_ready_s"], 10.4);
        assert_eq!(timing["samples"][1]["scene_submitted_s"], 10.45);
        assert_eq!(timing["samples"][1]["accepted_before"], false);
        assert_ne!(timing["samples"][0]["witness"]["world"], timing["samples"][1]["witness"]["world"]);
        assert_ne!(preview(&app).unwrap().mesh.positions, first);
        assert_eq!(preview_revision(&app).unwrap().0, 1);
        assert!(!app.forms_prototype.extrusion.gesture.as_ref().unwrap().accepted);
        assert!(app.forms_prototype.extrusion.gesture.as_ref().unwrap().pending.is_some());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), source);
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), recovery);
        app.preferences.forms_mesh_while_dragging = false;
        assert!(preview(&app).is_none());
        app.preferences.forms_mesh_while_dragging = true;
        assert!(preview(&app).is_some());
        cancel(&mut app);
        assert!(preview(&app).is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), source);
    }
    #[test]
    fn pending_ghost_does_not_hover_other_faces_and_cancel_restores_hover() {
        let (mut app, face) = matched();
        app.forms_prototype.pick = Pick::Face;
        app.cam.target = Vec3::ZERO;
        app.cam.distance = 30.0;
        let mut h = crate::scenario::Harness::new();
        h.app = app;
        selection::sync(&mut h.app);
        h.app.home.open = false;
        h.frames(3);
        let source = h.app.session.forms_prototype.as_ref().unwrap().inspect();
        let document = h.app.session.doc.clone();
        let recovery = solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap();
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let center = proj.to_screen(super::triad(&h.app).unwrap().center).unwrap();
        let form = h.app.session.forms_prototype.as_ref().unwrap();
        let mesh = &form.surface.mesh;
        let other = mesh
            .triangles
            .iter()
            .enumerate()
            .filter(|(i, _)| mesh.tri_face[*i] as usize != face)
            .filter_map(|(_, triangle)| {
                let point = triangle.iter().map(|&i| mesh.positions[i as usize]).fold(Vec3::ZERO, |a, b| a + b) / 3.0;
                proj.to_screen(point)
            })
            .filter(|point| proj.rect.contains(*point) && point.distance(center) > 110.0)
            .find_map(|point| match surface::nearest(&h.app, &proj, point) {
                Some(Element::Face(other)) if other != face => Some((point, other)),
                _ => None,
            })
            .unwrap();
        request(&mut h.app, face, 0.5);
        let _held_qualification = delayed_feedback(&mut h.app);
        poll(&mut h.app);
        assert!(preview(&h.app).is_some());
        h.app.forms_prototype.hover = Some(Element::Face(other.1));
        h.frame_with(vec![egui::Event::PointerMoved(other.0)]);
        assert!(active(&h.app));
        assert!(preview(&h.app).is_some());
        assert_eq!(h.app.forms_prototype.hover, None, "the pending extrusion owns the pointer, including over unchanged faces");
        assert_eq!(h.app.forms_prototype.elements, [Element::Face(face)].into_iter().collect());
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().inspect(), source);
        assert_eq!(h.app.session.doc, document);
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap(), recovery);
        release(&mut h.app);
        h.frame_with(vec![egui::Event::PointerMoved(other.0)]);
        assert!(active(&h.app));
        assert_eq!(h.app.forms_prototype.hover, None, "release still waits for qualification");
        cancel(&mut h.app);
        h.frame_with(vec![egui::Event::PointerMoved(other.0)]);
        assert_eq!(h.app.forms_prototype.hover, Some(Element::Face(other.1)), "ordinary hover resumes after cancellation");
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().inspect(), source);
    }
    #[test]
    fn extrusion_input_observations_are_bounded_and_do_not_author_state() {
        let (mut app, face) = matched();
        let source = app.session.forms_prototype.as_ref().unwrap().inspect();
        for index in 0..300 {
            app.now = index as f64;
            request(&mut app, face, 0.2 + index as f64 * 0.001);
        }
        let timing = inspect(&app)["timing"].clone();
        let samples = timing["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 256);
        assert_eq!(samples[0]["sequence"], 44);
        assert_eq!(samples[255]["sequence"], 299);
        assert!(samples.iter().all(|sample| sample["scene_submitted_s"].is_null() && sample["witness"].is_null()));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), source);
    }
    #[test]
    fn latest_failure_and_stale_source_remove_ghost_and_keep_the_qualified_draft() {
        for stale in [false, true] {
            let (mut app, face) = matched();
            request(&mut app, face, 0.2);
            let delay = delayed_feedback(&mut app);
            poll(&mut app);
            assert!(preview(&app).is_some());
            if stale {
                app.session.execute("form.prototype.finish_options", &json!({"tolerance":"0.02 mm"})).unwrap();
                assert!(preview(&app).is_none(), "render must hide stale feedback before the next poll");
            } else {
                delay.send(Err("Match qualification failed".into())).unwrap();
            }
            let valid = app.session.forms_prototype.as_ref().unwrap().inspect();
            release(&mut app);
            poll(&mut app);
            assert!(preview(&app).is_none());
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), valid);
        }
    }
    #[test]
    fn a_document_switch_discards_ghost_but_keeps_the_qualified_worker_contract() {
        let (mut app, face) = matched();
        let document = app.session.document_id.clone();
        request(&mut app, face, 0.2);
        let _delay = delayed_feedback(&mut app);
        poll(&mut app);
        assert!(preview(&app).is_some());
        crate::documents::new_design(&mut app);
        assert!(preview(&app).is_none());
        crate::documents::switch(&mut app, 0);
        assert_eq!(app.session.document_id, document);
        assert!(active(&app));
        assert!(preview(&app).is_none());
        assert!(app.forms_prototype.extrusion.gesture.as_ref().unwrap().response.is_none());
    }
    #[test]
    fn a_failed_new_target_discards_ghost_and_keeps_the_last_qualified_state_and_one_undo() {
        let (mut app, face) = matched();
        let original = app.session.forms_prototype.as_ref().unwrap().definition();
        let depth = app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
        request(&mut app, face, 0.2);
        let _delay = delayed_feedback(&mut app);
        poll(&mut app);
        assert!(preview(&app).is_some());
        ready(&mut app);
        poll(&mut app);
        assert!(preview(&app).is_none());
        let qualified = app.session.forms_prototype.as_ref().unwrap().inspect();
        assert_eq!(qualified["undo"].as_u64().unwrap(), depth + 1);
        request(&mut app, face, 0.8);
        assert!(preview(&app).is_some(), "prepared feedback follows the new target without another solve");
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Err("Match qualification failed".into())).unwrap();
        let gesture = app.forms_prototype.extrusion.gesture.as_mut().unwrap();
        gesture.pending = Some(Pending { preview: None, sequence: gesture.sequence, done: rx });
        release(&mut app);
        poll(&mut app);
        assert!(preview(&app).is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), qualified);
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), original);
    }
    #[test]
    fn queued_extrusion_follows_its_document_tab() {
        let (mut app, face) = matched();
        let original = app.session.document_id.clone();
        request(&mut app, face, 0.5);
        ready(&mut app);
        release(&mut app);
        crate::documents::new_design(&mut app);
        assert_ne!(app.session.document_id, original);
        assert!(!active(&app));
        poll(&mut app);
        assert!(app.session.forms_prototype.is_none());
        crate::documents::switch(&mut app, 0);
        assert_eq!(app.session.document_id, original);
        assert!(active(&app));
        poll(&mut app);
        assert!(!active(&app));
        assert!(app.session.forms_prototype.as_ref().unwrap().hierarchy.is_some());
    }
}
