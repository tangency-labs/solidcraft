//! `ui.screenshot`: a picture of the window as it is presented, for a control request. The
//! window asks egui for a frame a few frames after the request (a view that just moved settles
//! first), saves it where the request says and answers; a hidden window, a request whose agent
//! chat turn ended, or no frame within 8 s is answered at once with why.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::ColorImage;
use serde_json::{Value, json};
use solidcraft_control::{cancelled_chat_response, chat_turn_cancelled};

use crate::window::hidden;

/// A frame asked for: its token, the time (ms) from which it may be taken, and the frames drawn
/// since it was asked for.
pub type Queued = (u64, f64, u32);

/// A request waiting for its frame: its token, where to save it, where to answer, and the time
/// (ms) after which no frame is coming.
pub type Pending = (u64, Option<String>, Sender<Value>, f64);

/// A frame egui handed back, by its token.
pub type Shot = (u64, Arc<ColorImage>);

/// The window's screenshots in flight. The app may queue frames of its own (an issue report's)
/// with tokens from [`Shots::token`]; [`Shots::collect`] hands those back.
pub struct Shots {
    /// The error a request gets when the window presents no frame: the app's words, which may
    /// name its other ways to a picture.
    no_frame: String,
    /// The last token given out.
    pub token: u64,
    /// Frames asked for and not yet requested from egui.
    pub queued: Vec<Queued>,
    /// Control requests waiting for their frame.
    pub pending: Vec<Pending>,
    /// The agent chat turn of each pending request that belongs to one.
    turns: HashMap<u64, Arc<AtomicBool>>,
    /// Frames egui hands back; egui calls back, [`Shots::collect`] reads them.
    tx: Sender<Shot>,
    rx: Receiver<Shot>,
}

impl Shots {
    /// No screenshots in flight; a request the window presents no frame for is answered with the
    /// error `no_frame`.
    pub fn new(no_frame: impl Into<String>) -> Self {
        let (tx, rx) = channel();
        Shots { no_frame: no_frame.into(), token: 0, queued: Vec::new(), pending: Vec::new(), turns: HashMap::new(), tx, rx }
    }

    /// Take the window's picture for a request that answers on `reply`, saved at `path`; `turn`:
    /// the agent chat turn it belongs to. `now` is in ms. A window that is minimised or covered
    /// (another macOS Space) presents no frame: that is said at once rather than after the
    /// deadline (#1677).
    pub fn request(&mut self, ctx: &egui::Context, path: Option<String>, reply: Sender<Value>, turn: Option<Arc<AtomicBool>>, now: f64) {
        if hidden(ctx) {
            let _ = reply.send(json!({"ok": false, "hidden": true, "error": self.no_frame}));
            return;
        }
        self.token += 1;
        let token = self.token;
        if let Some(turn) = turn {
            self.turns.insert(token, turn);
        }
        self.queued.push((token, now + 150.0, 0));
        self.pending.push((token, path, reply, now + 8000.0));
    }

    /// Each frame: ask egui for the frames that are due, three frames after their request at the
    /// earliest. `settled`: the view is drawn at full quality; a view that keeps moving is taken
    /// as it is half a second later (#7165).
    pub fn issue(&mut self, ctx: &egui::Context, settled: bool, now: f64) {
        let shot_tx = &self.tx;
        self.queued.retain_mut(|(token, at, frames)| {
            *frames += 1;
            if now >= *at && *frames >= 3 && (settled || now >= *at + 500.0) {
                let (token, tx, ctx2) = (*token, shot_tx.clone(), ctx.clone());
                ctx.request_screenshot(move |image| {
                    let _ = tx.send((token, image));
                    ctx2.request_repaint();
                });
                false
            } else {
                true
            }
        });
        if !self.queued.is_empty() || !self.pending.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    /// Each frame: answer the requests whose frame came, saving it with `save` (the reply), and
    /// those whose turn ended or that wait in vain. `others`: the app waits for frames of its own;
    /// then every frame that came this frame is handed back, for the app to take its own from.
    pub fn collect(&mut self, ctx: &egui::Context, now: f64, others: bool, save: impl Fn(&ColorImage, Option<&str>) -> Value) -> Vec<Shot> {
        // A screenshot requested in one tab must never return or save a frame of another.
        let turns = &mut self.turns;
        let queued = &mut self.queued;
        self.pending.retain(|(token, _, reply, _)| {
            if chat_turn_cancelled(turns.get(token).map(Arc::as_ref)) {
                turns.remove(token);
                queued.retain(|(waiting, ..)| waiting != token);
                let _ = reply.send(cancelled_chat_response());
                false
            } else {
                true
            }
        });
        // Drained every frame: a shot that comes after its request was cancelled or timed out
        // is dropped here instead of waiting in the channel.
        let shots: Vec<Shot> = self.rx.try_iter().collect();
        if self.pending.is_empty() && !others {
            return Vec::new();
        }
        for (token, image) in &shots {
            if let Some(i) = self.pending.iter().position(|(t, ..)| t == token) {
                let (_, path, reply, _) = self.pending.remove(i);
                self.turns.remove(token);
                let _ = reply.send(save(image, path.as_deref()));
            }
        }
        // Hidden while waiting: no frame is coming.
        let hidden = hidden(ctx);
        let (turns, no_frame) = (&mut self.turns, &self.no_frame);
        self.pending.retain(|(token, _, reply, deadline)| {
            if now < *deadline && !hidden {
                return true;
            }
            let _ = reply.send(json!({"ok": false, "error": no_frame}));
            turns.remove(token);
            false
        });
        shots
    }
}

/// `image` in the format `path`'s extension names (a PNG for `.png`).
pub fn encode(image: &ColorImage, path: &str) -> Result<Vec<u8>, String> {
    let [w, h] = image.size;
    let mut buf = Vec::with_capacity(w * h * 4);
    for c in &image.pixels {
        buf.extend_from_slice(&c.to_array());
    }
    let img = image::RgbaImage::from_raw(w as u32, h as u32, buf).ok_or("bad image")?;
    let format = image::ImageFormat::from_path(path).map_err(|e| e.to_string())?;
    let mut bytes = std::io::Cursor::new(Vec::new());
    img.write_to(&mut bytes, format).map_err(|e| e.to_string())?;
    Ok(bytes.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app's reply for no frame.
    const NO_FRAME: &str = "no frame here";

    fn pixel_image() -> Arc<ColorImage> {
        Arc::new(ColorImage::new([4, 3], vec![egui::Color32::from_rgb(10, 20, 30); 12]))
    }

    /// A request is answered with its frame once egui hands it back, saved as the app saves it;
    /// a frame of the app's own is handed back to it; a request whose frame never comes is
    /// answered after its deadline.
    #[test]
    fn a_request_is_answered_with_its_frame_or_its_deadline() {
        let ctx = egui::Context::default();
        let mut shots = Shots::new(NO_FRAME);
        let (reply, answer) = channel();
        shots.request(&ctx, Some("a.png".into()), reply, None, 0.0);
        assert_eq!((shots.token, shots.queued.len(), shots.pending.len()), (1, 1, 1));
        // The frame egui hands back, and one the app asked for itself.
        shots.queued.clear();
        let _ = shots.tx.send((1, pixel_image()));
        let _ = shots.tx.send((2, pixel_image()));
        let save = |image: &ColorImage, path: Option<&str>| json!({"ok": true, "result": {"path": path, "width": image.size[0]}});
        let back = shots.collect(&ctx, 1.0, true, save);
        assert_eq!(back.iter().map(|(t, _)| *t).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(answer.try_recv().ok(), Some(json!({"ok": true, "result": {"path": "a.png", "width": 4}})));
        assert!(shots.pending.is_empty());

        let (reply, late) = channel();
        shots.request(&ctx, None, reply, None, 0.0);
        assert!(shots.collect(&ctx, 7999.0, false, save).is_empty());
        assert!(late.try_recv().is_err(), "still waiting");
        shots.collect(&ctx, 8000.0, false, save);
        assert_eq!(late.try_recv().ok(), Some(json!({"ok": false, "error": NO_FRAME})));
    }

    /// A request whose agent chat turn ended is answered so, and its frame is never taken.
    #[test]
    fn a_request_whose_turn_ended_is_refused() {
        let ctx = egui::Context::default();
        let mut shots = Shots::new(NO_FRAME);
        let (reply, answer) = channel();
        let turn = Arc::new(AtomicBool::new(true));
        shots.request(&ctx, None, reply, Some(Arc::clone(&turn)), 0.0);
        turn.store(false, std::sync::atomic::Ordering::Relaxed);
        shots.collect(&ctx, 1.0, false, |_, _| Value::Null);
        assert_eq!(answer.try_recv().ok(), Some(cancelled_chat_response()));
        assert!(shots.queued.is_empty() && shots.pending.is_empty());
    }

    /// The saved bytes are the format the path names.
    #[test]
    fn a_png_path_gets_a_png() {
        let bytes = encode(&pixel_image(), "shot.png").unwrap();
        assert!(bytes.starts_with(b"\x89PNG"), "{:?}", bytes.get(..8));
        assert!(encode(&pixel_image(), "shot.nope").is_err());
    }
}
