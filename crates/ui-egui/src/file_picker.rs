//! Native pickers must return to the event loop while the user chooses a file (#1088).
//! The host opens an async sheet; its continuation runs on the UI thread once it answers.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use serde_json::{Value, json};

use crate::SolveApp;

/// A host picker answer: a selected path, or `None` when cancelled.
pub type FilePickerFuture = Pin<Box<dyn Future<Output = Option<String>>>>;
pub(crate) type Completion = Box<dyn FnOnce(&mut SolveApp, Option<String>)>;

pub(crate) struct Pending {
    future: FilePickerFuture,
    complete: Completion,
}

struct Repaint(egui::Context);

impl Wake for Repaint {
    fn wake(self: Arc<Self>) {
        self.0.request_repaint();
    }
}

impl SolveApp {
    /// Keep the target design and editors unchanged until the picker answers.
    /// (A save being written keeps it unchanged too, see [`crate::save_job`].)
    pub fn file_picker_busy(&self) -> bool {
        crate::webdav::busy(self) || self.file_picker.is_some() || self.saving.is_some() || self.host_save.is_some() || self.disk_prompt
    }

    pub(crate) fn pick_open(&mut self, complete: impl FnOnce(&mut SolveApp, Option<String>) + 'static) {
        self.pick_open_for(&[], complete);
    }

    /// Open a file of this type: the picker offers these extensions first (#1801).
    pub(crate) fn pick_open_for(&mut self, extensions: &[&str], complete: impl FnOnce(&mut SolveApp, Option<String>) + 'static) {
        if self.file_picker_busy() {
            return;
        }
        let complete = match crate::webdav::pick(self, None, extensions, Box::new(complete)) {
            Ok(()) => return,
            Err(complete) => complete,
        };
        let future = self.services.pick_open.as_ref().map(|pick| pick(extensions));
        self.begin_picker(future, Box::new(complete));
    }

    /// Ask for one file of a kind (`label`, `extensions`), with the host's dialog for it, or the
    /// Open dialog where the host has none. (The agent chat asks for its files; the web build has
    /// no chat.)
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn pick_file(&mut self, label: &str, extensions: &[&str], complete: impl FnOnce(&mut SolveApp, Option<String>) + 'static) {
        if self.file_picker_busy() {
            return;
        }
        let future = match self.services.pick_file.as_ref() {
            Some(pick) => Some(pick(label, extensions)),
            None => self.services.pick_open.as_ref().map(|pick| pick(extensions)),
        };
        self.begin_picker(future, Box::new(complete));
    }

    pub(crate) fn pick_save(&mut self, name: &str, extensions: &[&str], complete: impl FnOnce(&mut SolveApp, Option<String>) + 'static) {
        if self.file_picker_busy() {
            return;
        }
        let complete = match crate::webdav::pick(self, Some(name), extensions, Box::new(complete)) {
            Ok(()) => return,
            Err(complete) => complete,
        };
        let future = self.services.pick_save.as_ref().map(|pick| pick(name, extensions));
        self.begin_picker(future, Box::new(complete));
    }

    /// Ask for a program (a slicer). Without a host dialog, the answer is a cancel.
    pub(crate) fn pick_program(&mut self, complete: impl FnOnce(&mut SolveApp, Option<String>) + 'static) {
        if self.file_picker_busy() {
            return;
        }
        let future = self.services.pick_program.as_ref().map(|pick| pick());
        self.begin_picker(future, Box::new(complete));
    }

    pub(crate) fn begin_webdav_fallback(&mut self, future: Option<FilePickerFuture>, complete: Completion) {
        self.begin_picker(future, complete);
    }

    fn begin_picker(&mut self, future: Option<FilePickerFuture>, complete: Completion) {
        let Some(future) = future else {
            complete(self, None);
            return;
        };
        self.file_picker = Some(Pending { future, complete });
    }

    pub(crate) fn pick_open_command(&mut self, command: &str, params: Value, extensions: &[&str]) {
        let command = command.to_string();
        self.pick_open_for(extensions, move |app, path| {
            if let Some(path) = path {
                let mut params = params;
                params["path"] = json!(path);
                let result = app.run(&command, params);
                if command == "parameters.import"
                    && let Some(dialog) = app.params.as_mut()
                {
                    dialog.error = result.err();
                }
            }
        });
    }

    pub(crate) fn pick_save_command(&mut self, name: &str, extensions: &[&str], command: &str, params: Value) {
        let command = command.to_string();
        self.pick_save(name, extensions, move |app, path| {
            if let Some(path) = path {
                let mut params = params;
                params["path"] = json!(path);
                let result = app.run(&command, params);
                if command == "parameters.export"
                    && let Some(dialog) = app.params.as_mut()
                {
                    dialog.error = result.err();
                }
            }
        });
    }
}

pub(crate) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(pending) = app.file_picker.as_mut() else { return };
    let waker = Waker::from(Arc::new(Repaint(ctx.clone())));
    let Poll::Ready(path) = pending.future.as_mut().poll(&mut Context::from_waker(&waker)) else {
        // A fallback for native backends that do not wake reliably. Resizing still draws
        // the ordinary UI in every frame.
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        return;
    };
    if let Some(pending) = app.file_picker.take() {
        (pending.complete)(app, path);
        // Hosts can flush writes or restore native input on the following frame.
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[test]
    fn webdav_listing_and_connection_bound_untrusted_input() {
        use crate::webdav::*;
        let base = connection("https://solidcraft.io/dav/").unwrap();
        let response = |href: &str, properties: &str| {
            format!(
                "<d:response><d:href>{href}</d:href><d:propstat><d:prop>{properties}</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
            )
        };
        let xml = |body: &str| format!("<d:multistatus xmlns:d=\"DAV:\">{body}</d:multistatus>");
        let body = [
            response("/dav/", "<d:resourcetype><d:collection/></d:resourcetype>"),
            response("/dav/Parts/", "<d:resourcetype><d:collection/></d:resourcetype>"),
            response("/dav/a%20%26b.solidcraft", "<d:getcontentlength>12</d:getcontentlength>"),
        ]
        .concat();
        assert_eq!(
            listing(xml(&body).as_bytes(), &base).unwrap(),
            vec![Entry { name: "Parts".into(), directory: true }, Entry { name: "a &b.solidcraft".into(), directory: false }]
        );
        assert!(listing(xml(&response("https://solidcraft.io/dav/a.solidcraft", "")).as_bytes(), &base).is_ok());
        for href in [
            "/dav/../private.solidcraft",
            "/dav/%2e%2e/private.solidcraft",
            "/dav/a%2fb.solidcraft",
            "/dav/a%5cb.solidcraft",
            "https://other.solidcraft.io/dav/a.solidcraft",
            "/dav/deep/a.solidcraft",
        ] {
            assert!(listing(xml(&response(href, "")).as_bytes(), &base).is_err(), "{href}");
        }
        assert!(listing(&vec![b' '; MAX_LIST_BYTES + 1], &base).is_err());
        assert!(
            listing(xml(&response("/dav/a.solidcraft", "<d:getcontentlength>18446744073709551615</d:getcontentlength>")).as_bytes(), &base).is_err()
        );
        assert!(
            listing(xml(&(0..MAX_ENTRIES + 2).map(|i| response(&format!("/dav/a{i}.solidcraft"), "")).collect::<String>()).as_bytes(), &base)
                .is_err()
        );
        for hostile in [
            "<!DOCTYPE x [<!ENTITY x 'bad'>]><d:multistatus xmlns:d=\"DAV:\"/>",
            "<multistatus xmlns='DAV:'><a></multistatus>",
            "<multistatus xmlns='DAV:'><a>",
            "<multistatus xmlns='other'/>",
        ] {
            assert!(listing(hostile.as_bytes(), &base).is_err());
        }
        assert!(listing(format!("<multistatus xmlns='DAV:'>{}{}</multistatus>", "<a>".repeat(33), "</a>".repeat(33)).as_bytes(), &base).is_err());
        for value in [
            "http://solidcraft.io/dav/",
            "ftp://localhost/dav/",
            "https://user:password@solidcraft.io/dav/",
            "https://solidcraft.io/dav/?token=secret",
            "http://localhost.solidcraft.io/",
        ] {
            assert!(connection(value).is_err(), "{value}");
        }
        for value in ["http://localhost:8080/dav/", "http://127.0.0.1/dav/", "http://[::1]:8080/dav/", "https://solidcraft.io/dav/"] {
            assert!(connection(value).is_ok(), "{value}");
        }
    }

    #[derive(Default)]
    struct Answer {
        path: RefCell<Option<Option<String>>>,
        polls: Cell<usize>,
        waker: RefCell<Option<Waker>>,
    }

    struct Delayed(Rc<Answer>);
    impl Future for Delayed {
        type Output = Option<String>;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            self.0.polls.set(self.0.polls.get() + 1);
            *self.0.waker.borrow_mut() = Some(cx.waker().clone());
            self.0.path.borrow_mut().take().map_or(Poll::Pending, Poll::Ready)
        }
    }

    fn picker(app: &mut SolveApp, save: bool) -> Rc<Answer> {
        let answer = Rc::new(Answer::default());
        let captured = answer.clone();
        if save {
            app.services.pick_save = Some(Box::new(move |_, _| Box::pin(Delayed(captured.clone()))));
        } else {
            app.services.pick_open = Some(Box::new(move |_| Box::pin(Delayed(captured.clone()))));
        }
        answer
    }

    fn finish(app: &mut SolveApp, answer: &Answer, path: Option<String>) {
        *answer.path.borrow_mut() = Some(path);
        if let Some(waker) = answer.waker.borrow_mut().take() {
            waker.wake();
        }
        tick(app, &crate::fonts::test_context());
        // A save the answer started is written on a worker: wait for it.
        crate::save_job::tick(app, true);
        assert!(!app.file_picker_busy());
    }

    /// #1088: Open and Save stay pending through real UI frames at changing window sizes.
    /// Model input and tab changes wait, while the event loop keeps painting.
    #[test]
    fn pending_open_and_save_allow_resize_frames_without_changing_the_design() {
        for save in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
            let before = h.app.session.doc.to_json();
            let answer = picker(&mut h.app, save);
            let completed = Rc::new(Cell::new(0));
            let count = completed.clone();
            let callback = move |_: &mut SolveApp, path: Option<String>| {
                assert!(path.is_none());
                count.set(count.get() + 1);
            };
            if save {
                h.app.pick_save("part.solidcraft", &["solidcraft"], callback);
            } else {
                h.app.pick_open(callback);
            }
            assert_eq!(answer.polls.get(), 0, "the UI action returns before polling");
            for (width, height) in [(1200, 800), (1600, 1000), (960, 600), (1400, 900)] {
                h.call("ui.resize", json!({"width":width,"height":height}));
                h.frames(2);
                assert!(h.app.file_picker_busy());
                assert_eq!(completed.get(), 0);
                assert_eq!(h.ctx.content_rect().width(), width as f32);
            }
            assert!(answer.polls.get() >= 8);
            crate::documents::new_design(&mut h.app);
            crate::documents::close(&mut h.app, 0, true);
            crate::documents::request_quit(&mut h.app);
            h.app.start("solid.box");
            assert!(h.app.run("solid.box", json!({})).is_err());
            h.step(&json!({"key":"N", "cmd":true})).unwrap();
            assert_eq!(h.app.docs.count(), 1);
            assert!(!h.app.quit_requested);
            assert_eq!(h.app.session.doc.to_json(), before);
            // Duplicate requests cannot replace the pending operation.
            h.app.pick_open(|_, _| panic!("duplicate picker"));
            finish(&mut h.app, &answer, None);
            tick(&mut h.app, &h.ctx);
            assert_eq!(completed.get(), 1);
            assert_eq!(h.app.session.doc.to_json(), before);
        }
    }

    #[test]
    fn save_as_waits_for_its_path_and_saves_only_on_completion() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        let answer = picker(&mut h.app, true);
        let path = h.dir.join("chosen.solidcraft");
        crate::toolbar::save(&mut h.app);
        h.frames(3);
        assert!(h.app.session.is_dirty() && !path.exists());
        finish(&mut h.app, &answer, Some(path.to_string_lossy().into_owned()));
        assert!(path.is_file() && !h.app.session.is_dirty());
        assert_eq!(h.app.session.path.as_deref(), path.to_str());
    }

    /// The sheet-metal Output buttons must ask where to save, not discard the headless
    /// command's returned text (#6847). Cancelling must not export or change the design.
    #[test]
    fn sheet_exports_wait_for_a_path_and_write_only_on_completion() {
        for (command, extension, field) in [("sheet.export_dxf", "dxf", "dxf"), ("sheet.export_bend_table", "csv", "text")] {
            let mut h = crate::scenario::Harness::new();
            for (id, params) in [
                ("sketch.create", json!({"plane": "XY", "name": "Base"})),
                ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]})),
                ("sketch.finish", json!({})),
                ("sheet.flange", json!({"sketch": "Base"})),
                ("sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": "20 mm", "position": "outside"})),
            ] {
                h.app.run(id, params).unwrap();
            }
            let answer = picker(&mut h.app, true);
            let before = h.app.session.doc.to_json();
            let expected = h.app.session.execute(command, &json!({})).unwrap();
            assert!(!h.app.file_picker_busy(), "programmatic calls return text without a picker");
            let text = expected[field].as_str().unwrap();
            assert!(text.contains(if extension == "dxf" { "BEND_UP" } else { "Body1" }));
            let path = h.dir.join(format!("flat.{extension}"));
            for outcome in ["cancel", "fail", "save"] {
                h.app.start(command);
                h.frames(2);
                assert!(h.app.file_picker_busy(), "{command} must open the save picker");
                assert!(!path.exists());
                assert_eq!(h.app.session.doc.to_json(), before);
                let chosen = match outcome {
                    "save" => Some(path.clone()),
                    "fail" => Some(h.dir.join("missing").join(format!("flat.{extension}"))),
                    _ => None,
                };
                finish(&mut h.app, &answer, chosen.map(|p| p.to_string_lossy().into_owned()));
                assert_eq!(path.exists(), outcome == "save");
                if outcome == "fail" {
                    assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
                }
                assert_eq!(h.app.session.doc.to_json(), before);
            }
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
            assert!(h.app.status.as_ref().is_some_and(|(text, _, error)| text.starts_with("Exported ") && !error));
        }
    }

    /// Saving the design says where it went, as an export does (#8693).
    #[test]
    fn saving_the_design_says_where_it_went() {
        let mut h = crate::scenario::Harness::new();
        let path = h.dir.join("Bracket.sc").to_string_lossy().into_owned();
        h.app.run("file.save", json!({"path": path})).unwrap();
        assert!(h.app.status.as_ref().is_some_and(|(text, _, error)| *text == format!("Saved {path}") && !error), "{:?}", h.app.status);
        // A save that fails says so instead.
        let missing = h.dir.join("missing").join("Bracket.sc").to_string_lossy().into_owned();
        assert!(h.app.run("file.save", json!({"path": missing})).is_err());
        assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
    }

    #[test]
    fn sheet_exports_without_a_sheet_report_an_error_without_a_picker() {
        let mut h = crate::scenario::Harness::new();
        let asked = Rc::new(Cell::new(0));
        let count = asked.clone();
        h.app.services.pick_save = Some(Box::new(move |_, _| {
            count.set(count.get() + 1);
            Box::pin(std::future::ready(None))
        }));
        for command in ["sheet.export_dxf", "sheet.export_bend_table"] {
            h.app.start(command);
            assert_eq!(asked.get(), 0);
            assert!(!h.app.file_picker_busy());
            assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
        }
    }

    #[test]
    fn close_and_quit_wait_for_save_as_and_cancel_or_failure_keeps_changes() {
        for quit in [false, true] {
            for outcome in ["save", "cancel", "fail"] {
                let mut h = crate::scenario::Harness::new();
                h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
                let before = h.app.session.doc.to_json();
                let answer = picker(&mut h.app, true);
                if quit {
                    crate::documents::request_quit(&mut h.app);
                } else {
                    crate::documents::close(&mut h.app, 0, false);
                }
                crate::documents::answer_prompt(&mut h.app, crate::documents::Answer::Save);
                h.frames(3);
                assert!(h.app.file_picker_busy() && !h.app.quit_requested);
                assert_eq!(h.app.docs.closing, Some(0));
                let path = match outcome {
                    "save" => Some(h.dir.join("chosen.solidcraft")),
                    "fail" => Some(h.dir.join("missing/chosen.solidcraft")),
                    _ => None,
                };
                finish(&mut h.app, &answer, path.map(|p| p.to_string_lossy().into_owned()));
                if outcome == "save" {
                    assert_eq!(h.app.quit_requested, quit);
                    assert!(!h.app.session.is_dirty());
                    if !quit {
                        assert!(h.app.home.open && h.app.session.doc.features.is_empty());
                    }
                } else {
                    assert!(!h.app.quit_requested && h.app.session.is_dirty());
                    assert!(h.app.docs.quitting.is_none() && h.app.docs.closing.is_none());
                    assert_eq!(h.app.session.doc.to_json(), before);
                }
            }
        }
    }

    #[test]
    fn quit_saves_each_untitled_tab_after_its_own_dialog_answers() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        crate::documents::new_design(&mut h.app);
        h.app.run("solid.box", json!({"length":20,"width":20,"height":20})).unwrap();
        crate::documents::request_quit(&mut h.app);
        for i in 0..2 {
            let answer = picker(&mut h.app, true);
            crate::documents::answer_prompt(&mut h.app, crate::documents::Answer::Save);
            h.frames(2);
            assert!(!h.app.quit_requested);
            finish(&mut h.app, &answer, Some(h.dir.join(format!("tab{i}.solidcraft")).to_string_lossy().into_owned()));
            assert_eq!(h.app.quit_requested, i == 1);
        }
        assert!(!crate::documents::any_dirty(&h.app));
    }

    /// #1801: Insert SVG and Insert DXF ask the host for their own file type first; other
    /// opens keep the default list.
    #[test]
    fn sketch_inserts_ask_for_their_file_type() {
        let mut h = crate::scenario::Harness::new();
        let asked: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
        let log = asked.clone();
        h.app.services.pick_open = Some(Box::new(move |extensions| {
            log.borrow_mut().push(extensions.iter().map(|e| e.to_string()).collect());
            Box::pin(std::future::ready(None))
        }));
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        for (command, want) in [
            ("sketch.insert_svg", vec!["svg"]),
            ("sketch.insert_dxf", vec!["dxf"]),
            ("file.insert_step", vec![]),
            ("canvas.insert", vec!["png", "jpg", "jpeg"]),
            ("canvas.decal", vec!["png", "jpg", "jpeg"]),
        ] {
            h.app.start(command);
            tick(&mut h.app, &h.ctx);
            assert_eq!(asked.borrow().last().cloned(), Some(want.iter().map(|e| e.to_string()).collect::<Vec<_>>()), "{command}");
        }
    }

    #[test]
    fn picker_holds_engine_requests_but_answers_window_resize_requests() {
        let mut h = crate::scenario::Harness::new();
        let answer = picker(&mut h.app, false);
        h.app.pick_open(|_, _| {});
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.control_rx = Some(rx);
        let (command, reply) =
            crate::control::ControlRequest::new("engine.execute", json!({"command":"solid.box","params":{"length":10,"width":10,"height":10}}));
        let (resize, resized) = crate::control::ControlRequest::new("ui.resize", json!({"width":1200,"height":800}));
        let (home, shown) = crate::control::ControlRequest::new("ui.home", json!({"open":true}));
        tx.send(command).unwrap();
        tx.send(resize).unwrap();
        tx.send(home).unwrap();
        h.frames(2);
        assert!(reply.try_recv().is_err());
        assert_eq!(resized.try_recv().unwrap()["ok"], true);
        assert!(shown.try_recv().is_err());
        assert_eq!(h.call("ui.inspect", json!({}))["result"]["file_picker_busy"], true);
        assert!(h.app.session.doc.features.is_empty());
        finish(&mut h.app, &answer, None);
        h.frames(2);
        let result = reply.try_recv().unwrap();
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(shown.try_recv().unwrap()["ok"], true);
        assert_eq!(h.app.session.doc.features.len(), 1);
    }

    #[test]
    fn canvas_picker_starts_placement_only_after_a_file_is_selected() {
        let mut h = crate::scenario::Harness::new();
        let answer = picker(&mut h.app, false);
        h.app.start("canvas.insert");
        h.frames(2);
        assert!(h.app.tool.is_none());
        finish(&mut h.app, &answer, Some("selected-image.png".into()));
        assert_eq!(h.app.tool.as_ref().map(|tool| tool.cmd), Some("canvas.insert"));
        let answer = picker(&mut h.app, false);
        h.app.start("canvas.insert");
        finish(&mut h.app, &answer, None);
        assert!(h.app.tool.is_none(), "cancelling does not resume the old placement");
    }

    /// #3216: pickers for files that aren't designs list the files' own types. On macOS a type
    /// no filter names can't be picked: CSV parameters were greyed out.
    #[test]
    fn parameter_import_and_canvas_images_ask_for_their_file_types() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let asked: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
        let log = asked.clone();
        h.app.services.pick_open = Some(Box::new(move |types: &[&str]| {
            log.borrow_mut().push(types.iter().map(|t| t.to_string()).collect());
            Box::pin(std::future::ready(None))
        }));
        crate::params_dialog::open(&mut h.app);
        h.frames(2);
        h.step(&json!({"widget": "Import…"})).unwrap();
        h.frames(2);
        h.app.params = None;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.start("canvas.insert");
        h.frames(2);
        assert_eq!(*asked.borrow(), [vec!["csv", "json"], vec!["png", "jpg", "jpeg"]]);
    }

    /// #4717: a canvas placed with a click on the origin of a sketch on XY, seen from the top,
    /// lies on XY. The origin's XZ square is under the cursor there too, edge-on, and the image
    /// went onto it, out of sight.
    #[test]
    fn a_canvas_clicked_at_the_sketch_origin_lies_where_it_can_be_seen() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        for step in [
            json!({"make_image": "pic.png"}),
            json!({"choose_file": "pic.png"}),
            json!({"call": "ui.view", "params": {"view": "top"}}),
            json!({"start": "canvas.insert"}),
            json!({"click": {"world": [0, 0, 0]}}),
        ] {
            h.step(&step).unwrap();
        }
        let doc = &h.app.session.doc;
        let canvas = doc.canvases.first().expect("the canvas is placed");
        let (vals, _) = doc.param_values();
        let n = doc.canvas_plane(&vals, canvas).unwrap().normal();
        assert!(n.z.abs() > 0.99, "the canvas lies on {n:?}, not on XY");
    }

    /// A canvas and a decal clicked onto a face of a design with many curved faces land on it.
    /// The click sent only the point, and the engine's search of every face of every body for it
    /// gave up ("face capture work limit exceeded"), as on an imported trophy of nine bodies.
    #[test]
    fn a_canvas_and_a_decal_clicked_onto_a_face_of_a_large_design_land_on_it() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 200, "width": 120, "height": 40, "corner": [20, 20, 0]})).unwrap();
        for i in 0..8 {
            let center = [60 + 45 * (i % 4), 50 + 60 * (i / 4), 60];
            h.app.run("solid.torus", json!({"major": 30, "minor": 8, "center": center, "operation": "new_body"})).unwrap();
        }
        for command in ["canvas.insert", "canvas.decal"] {
            for step in [
                json!({"make_image": "pic.png"}),
                json!({"choose_file": "pic.png"}),
                json!({"call": "ui.view", "params": {"view": "iso"}}),
                json!({"start": command}),
                json!({"click": {"world": [30, 30, 40]}}),
            ] {
                h.step(&step).unwrap();
            }
        }
        let doc = &h.app.session.doc;
        assert_eq!(doc.canvases.len(), 2, "both images are placed");
        let (vals, _) = doc.param_values();
        for canvas in &doc.canvases {
            let plane = doc.canvas_plane(&vals, canvas).unwrap();
            assert!(plane.normal().z.abs() > 0.99 && (plane.origin.z - 40.0).abs() < 1e-6, "{} lies on {plane:?}", canvas.name);
        }
    }

    fn placement_hint_painted(h: &mut crate::scenario::Harness, hint: &str) -> bool {
        fn contains(shape: &egui::Shape, hint: &str) -> bool {
            match shape {
                egui::Shape::Text(text) => text.galley.job.text == hint,
                egui::Shape::Vec(shapes) => shapes.iter().any(|shape| contains(shape, hint)),
                _ => false,
            }
        }
        let out = h.ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() },
            |ui| h.app.ui(ui),
        );
        out.shapes.iter().any(|shape| contains(&shape.shape, hint))
    }

    fn image_placement(command: &str) -> (crate::scenario::Harness, &'static str) {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        if command == "canvas.decal" {
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
            h.app.fit_view();
        } else {
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        }
        let path = h.dir.join("image.png");
        let png = solidcraft_engine::render::png(1, 1, vec![255, 0, 0, 255]).unwrap();
        std::fs::write(&path, png).unwrap();
        let answer = picker(&mut h.app, false);
        h.app.start(command);
        h.frames(2);
        assert!(h.app.tool.is_none());
        finish(&mut h.app, &answer, Some(path.to_string_lossy().into_owned()));
        assert_eq!(h.app.tool.as_ref().map(|tool| tool.cmd), Some(command));
        let hint = if command == "canvas.decal" { "Click a face to place the decal" } else { "Click a plane or face to place the canvas" };
        assert!(placement_hint_painted(&mut h, hint), "{command}: placement instruction is visible");
        // An expired ordinary status must not dismiss the active placement instruction.
        h.app.status = Some(("Old status".into(), crate::now_ms() - 7000.0, false));
        assert!(placement_hint_painted(&mut h, hint));
        (h, hint)
    }

    #[test]
    fn image_placement_hint_clears_after_placement() {
        for command in ["canvas.insert", "canvas.decal"] {
            let (mut h, hint) = image_placement(command);
            let point = if command == "canvas.decal" { json!([5, 5, 10]) } else { json!([4, 3, 0]) };
            h.step(&json!({"click":{"world":point}})).unwrap();
            assert_eq!(h.app.session.doc.canvases.len(), 1, "{command}: {:?}", h.app.status);
            assert!(h.app.tool.is_none());
            assert!(!placement_hint_painted(&mut h, hint));
        }
    }

    #[test]
    fn image_placement_hint_clears_on_escape() {
        for command in ["canvas.insert", "canvas.decal"] {
            let (mut h, hint) = image_placement(command);
            h.step(&json!({"key":"Escape"})).unwrap();
            assert!(h.app.tool.is_none());
            assert!(h.app.session.doc.canvases.is_empty());
            assert!(!placement_hint_painted(&mut h, hint));
        }
    }
}
