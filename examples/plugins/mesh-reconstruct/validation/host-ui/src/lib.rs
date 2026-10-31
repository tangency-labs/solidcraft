//! Test-only consumer of the published host; no replacement runtime or TCP server.
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use serde_json::json;
    use solidcraft_engine::Session;
    use solidcraft_ui_egui::{ControlRequest, Services, SolveApp, scenario::Harness};
    use std::{path::PathBuf, sync::mpsc, time::Duration};

    #[test]
    fn real_component_stop_click_discards_adoption_and_allows_the_next_command() {
        let package = PathBuf::from(std::env::var("MESH_RECONSTRUCT_PACKAGE").expect("verified package path"));
        let source = PathBuf::from(std::env::var("MESH_PUBLIC_INPUT").expect("original procedural bracket input"));
        assert_eq!(source.file_name().unwrap(), "Bracket.stl");
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.agent_off_thread = true;
        h.app.session.execute("file.insert_mesh", &json!({"path":source})).unwrap();
        let mesh = h.app.session.execute("model.mesh", &json!({"body":"Bracket","tolerance":0.05})).unwrap();
        let ids: Vec<_> = h.app.session.doc.features.iter().map(|f| f.id).collect();
        let inspected = h.app.session.execute("plugin.inspect", &json!({"path":package})).unwrap();
        let (tx, rx) = mpsc::channel();
        let replacement = SolveApp::new(Session::default(), Services::default());
        h.app = std::mem::replace(&mut h.app, replacement).with_control(rx);
        h.frames(2);
        let (request, reply) = ControlRequest::new(
            "engine.execute",
            json!({
                "command":"plugin.run", "params":{"path":package,"approval":inspected["approval"],
                "entry":"convert","params":{"body":"Bracket","tolerance":0.05,"name":"Restored"}}
            }),
        );
        tx.send(request).unwrap();
        h.frames(1);
        assert!(h.app.agent_computing().is_some(), "must observe real background work before claiming cancellation");
        // Resolve the handle published by the actual rendered Stop widget.
        h.step(&json!({"click":{"handle":"agent:stop"}})).unwrap();
        h.frames(2);
        let stopped = reply.recv_timeout(Duration::from_secs(2)).expect("Stop must answer");
        assert_eq!(stopped["ok"], false, "{stopped}");
        assert_eq!(stopped["stopped"], true, "{stopped}");
        assert_eq!(h.app.session.doc.features.iter().map(|f| f.id).collect::<Vec<_>>(), ids);
        assert_eq!(h.app.session.execute("model.mesh", &json!({"body":"Bracket","tolerance":0.05})).unwrap(), mesh);
        // Observe later frames while the dropped worker may finish. No edit masks
        // late adoption by changing the document revision during this interval.
        for _ in 0..450 {
            h.frames(1);
            std::thread::sleep(Duration::from_millis(100));
            assert_eq!(h.app.session.doc.features.iter().map(|f| f.id).collect::<Vec<_>>(), ids);
        }
        assert_eq!(h.app.session.execute("inspect.measure", &json!({})).unwrap()["body_count"], 1);
        let (next, done) = ControlRequest::new(
            "engine.execute",
            json!({
                "command":"solid.box", "params":{"corner":[50,50,50],"length":1,"width":1,"height":1}
            }),
        );
        tx.send(next).unwrap();
        let mut answered = None;
        for _ in 0..500 {
            h.frames(1);
            if let Ok(r) = done.try_recv() {
                answered = Some(r);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let answered = answered.expect("next command completes after Stop");
        assert_eq!(answered["ok"], true, "{answered}");
        assert_eq!(h.app.session.execute("inspect.measure", &json!({})).unwrap()["body_count"], 2);
        assert_eq!(h.app.session.execute("model.mesh", &json!({"body":"Bracket","tolerance":0.05})).unwrap(), mesh);
    }
}
