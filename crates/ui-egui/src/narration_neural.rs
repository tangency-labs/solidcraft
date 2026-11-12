//! The natural neural voice for tutorial narration (native only): Kokoro synthesis on a background
//! thread, played through rodio. The model is bundled under `assets/tts/kokoro-en-q8/`.
//!
//! Synthesis takes too long for the UI thread, so a worker thread owns the engine and the audio
//! device and [`Neural::speak`] just hands it a line. Loading, synthesis and playback are all
//! third-party and are wrapped so a failure or panic there only drops narration — never the app
//! (the same care the rest of [`crate::narration`] takes). If the engine cannot load, the narrator
//! falls back to the operating system voice.
//!
//! The worker reports progress with two monotonic counters, `started` (lines that have begun
//! playing) and `done` (lines that have finished), so the tutorial player can pace the pointer to
//! the narration instead of running ahead of the slower synthesis ([`crate::tutorial`]).
//!
//! On wasm there is no model and no audio device here, so the whole thing is a stub that never
//! starts and the UI crate still builds for the browser.

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::num::NonZero;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
    use std::thread;
    use std::time::{Duration, Instant};

    use local_tts_core::{Language, SynthesisRequest, Synthesizer, kokoro::Kokoro};

    /// The bundled voice name.
    const SPEAKER: &str = "af_heart";
    /// The bundled package folder, relative to the repo root (development) or the executable
    /// (install).
    const PACKAGE: &str = "assets/tts/kokoro-en-q8";

    /// Worker state the narrator reads to decide whether to fall back to the system voice.
    const LOADING: u8 = 0;
    const READY: u8 = 1;
    const FAILED: u8 = 2;

    enum Msg {
        /// A line to speak, tagged with its caller-assigned sequence number.
        Speak(u64, String),
        Stop,
    }

    /// Progress the worker publishes, as caller-assigned line sequence numbers.
    struct Shared {
        state: AtomicU8,
        /// The sequence number of the latest line that has begun playing.
        started: AtomicU64,
        /// The sequence number of the latest line that has finished playing (or was cleared or
        /// failed).
        done: AtomicU64,
    }

    /// A handle to the neural voice worker. Dropping it ends the worker and stops playback.
    pub struct Neural {
        tx: Sender<Msg>,
        shared: Arc<Shared>,
        /// The next sequence number to hand out, assigned on the caller's thread so a line's number
        /// is known before the worker sees it (no race with the pacing in [`crate::tutorial`]).
        next: AtomicU64,
        _handle: thread::JoinHandle<()>,
    }

    impl Neural {
        /// Start the worker if the model is bundled; `None` when there is no model (the narrator
        /// then uses the system voice). The model loads on the worker thread, so this never blocks.
        pub fn start() -> Option<Neural> {
            let Some(dir) = model_dir() else {
            eprintln!("solidcraft: tutorial narration using the system voice (no bundled model found)");
            return None;
        };
            let (tx, rx) = mpsc::channel();
            let shared = Arc::new(Shared { state: AtomicU8::new(LOADING), started: AtomicU64::new(0), done: AtomicU64::new(0) });
            let worker_shared = Arc::clone(&shared);
            let handle = thread::Builder::new().name("solidcraft-narration".into()).spawn(move || worker(&dir, &rx, &worker_shared)).ok()?;
            Some(Neural { tx, shared, next: AtomicU64::new(0), _handle: handle })
        }

        /// Whether loading the engine or opening the audio device failed, so the narrator should use
        /// the system voice instead.
        pub fn failed(&self) -> bool {
            self.shared.state.load(Ordering::Relaxed) == FAILED
        }

        /// The sequence numbers of the latest lines that have begun and finished playing, for pacing
        /// the demo to the voice.
        pub fn progress(&self) -> (u64, u64) {
            (self.shared.started.load(Ordering::Relaxed), self.shared.done.load(Ordering::Relaxed))
        }

        /// Queue a line, first clearing anything queued or playing when `interrupt` is set. Returns
        /// the line's sequence number, which [`Self::progress`] reports back once it begins and
        /// finishes.
        pub fn speak(&self, text: &str, interrupt: bool) -> u64 {
            if interrupt {
                let _ = self.tx.send(Msg::Stop);
            }
            let seq = self.next.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = self.tx.send(Msg::Speak(seq, text.to_string()));
            seq
        }

        /// Stop anything being spoken and clear the queue.
        pub fn stop(&self) {
            let _ = self.tx.send(Msg::Stop);
        }
    }

    /// Find the bundled Kokoro package: an explicit override, then beside the executable
    /// (installed), then up from the executable or in the working directory (development). The
    /// first with a model wins.
    fn model_dir() -> Option<PathBuf> {
        let has_model = |p: &PathBuf| p.join("model.onnx").is_file();
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(dir) = std::env::var_os("SOLIDCRAFT_TTS_DIR") {
            candidates.push(PathBuf::from(dir));
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            candidates.push(dir.join("tts/kokoro-en-q8")); // installed: beside the executable
            candidates.push(dir.join("../..").join(PACKAGE)); // dev: target/<profile>/exe -> repo root
            candidates.push(dir.join("../../..").join(PACKAGE));
        }
        candidates.push(PathBuf::from(PACKAGE)); // dev: the working directory is the repo root
        candidates.into_iter().find(has_model)
    }

    /// The worker: load the engine and open the audio device, then synthesize and play each line
    /// while keeping the progress counters current, clearing on a stop. A failure sets `FAILED` and
    /// drains the queue so senders never block.
    fn worker(dir: &Path, rx: &Receiver<Msg>, shared: &Shared) {
        // The engine load (model + dictionary) is third-party; a panic there must not reach the
        // process.
        let engine = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut engine = Kokoro::load(dir).ok()?;
            engine.set_fallback(true).ok()?; // guess unknown words instead of erroring on any text
            Some(engine)
        }))
        .ok()
        .flatten();
        let Some(mut engine) = engine else { return drain_failed(rx, shared, "the model did not load") };
        let Ok(device) = rodio::DeviceSinkBuilder::open_default_sink() else {
            return drain_failed(rx, shared, "no audio device");
        };
        let player = rodio::Player::connect_new(device.mixer());
        shared.state.store(READY, Ordering::Relaxed);
        // Which voice is speaking is worth knowing from the log when narration sounds wrong.
        eprintln!("solidcraft: tutorial narration using the neural voice ({})", dir.display());

        // `playing_until` is when the current line's audio ends, so a timed-out wait marks it done;
        // `current` is that line's sequence number.
        let mut playing_until: Option<Instant> = None;
        let mut current: u64 = 0;
        loop {
            let msg = match playing_until {
                Some(end) => rx.recv_timeout(end.saturating_duration_since(Instant::now())),
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match msg {
                Ok(Msg::Stop) => {
                    player.clear();
                    if playing_until.take().is_some() {
                        shared.done.store(current, Ordering::Relaxed); // the cleared line is finished
                    }
                }
                Ok(Msg::Speak(seq, text)) => match synthesize(&mut engine, &text) {
                    Some((channels, rate, secs, samples)) => {
                        player.append(rodio::buffer::SamplesBuffer::new(channels, rate, samples));
                        player.play();
                        current = seq;
                        shared.started.store(seq, Ordering::Relaxed);
                        playing_until = Some(Instant::now() + Duration::from_secs_f64(secs.max(0.05)));
                    }
                    // Synthesis failed: mark the line started and done at once, so the demo never
                    // waits on a line that will not play.
                    None => {
                        shared.started.store(seq, Ordering::Relaxed);
                        shared.done.store(seq, Ordering::Relaxed);
                    }
                },
                Err(RecvTimeoutError::Timeout) => {
                    if playing_until.take().is_some() {
                        shared.done.store(current, Ordering::Relaxed); // the current line has finished
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        drop(device); // keep the device alive for the whole loop: dropping it stops playback
    }

    /// Mark the engine failed and drain the queue, so a sender never waits on a dead worker.
    fn drain_failed(rx: &Receiver<Msg>, shared: &Shared, why: &str) {
        shared.state.store(FAILED, Ordering::Relaxed);
        eprintln!("solidcraft: tutorial narration using the system voice ({why})");
        while rx.recv().is_ok() {}
    }

    /// Synthesize one line, guarded. Returns the mono channel count, the sample rate, the duration
    /// in seconds and the PCM samples, or `None` on any failure.
    fn synthesize(engine: &mut Kokoro, text: &str) -> Option<(NonZero<u16>, NonZero<u32>, f64, Vec<f32>)> {
        let audio = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            engine.synthesize(&SynthesisRequest { text, language: Language::En, speaker: SPEAKER, duration_scale: 1.0 }).ok()
        }))
        .ok()
        .flatten()?;
        let rate = NonZero::new(audio.sample_rate)?;
        let secs = audio.samples.len() as f64 / f64::from(audio.sample_rate);
        Some((NonZero::<u16>::MIN, rate, secs, audio.samples))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::Neural;

/// wasm has no bundled model and no audio device here: the voice never starts, so the narrator
/// uses its own stub and the UI crate still builds for the browser.
#[cfg(target_arch = "wasm32")]
pub struct Neural;

#[cfg(target_arch = "wasm32")]
impl Neural {
    pub fn start() -> Option<Neural> {
        None
    }
    pub fn failed(&self) -> bool {
        true
    }
    pub fn progress(&self) -> (u64, u64) {
        (0, 0)
    }
    pub fn speak(&self, _text: &str, _interrupt: bool) -> u64 {
        0
    }
    pub fn stop(&self) {}
}
