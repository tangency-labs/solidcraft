//! The microphone for dictation in the Assistant (#10883). It records the system's default input
//! device with cpal on a thread of its own: a cpal stream may not move between threads on every
//! system, so that thread opens it, keeps it until the recording ends and closes it. The samples
//! are mixed down to mono as they come; [`Recording::finish`] answers them as a 16 kHz WAV file.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use solidcraft_ui_egui::agent_chat::dictation::{self, Recorder, Recording};

/// Longest recording kept: ten minutes. Later samples are dropped.
const MAX_SECONDS: usize = 600;
/// How long the device may take to start.
const START_TIMEOUT: Duration = Duration::from_secs(5);

/// The system's default microphone.
pub struct Microphone;

impl Recorder for Microphone {
    fn start(&self) -> Result<Box<dyn Recording>, String> {
        let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let buffer = samples.clone();
        let thread = std::thread::Builder::new()
            .name("microphone".into())
            .spawn(move || match open(buffer) {
                Ok((stream, rate)) => {
                    drop(ready_tx.send(Ok(rate)));
                    // Until finish() or drop says stop (or the recording is gone).
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => drop(ready_tx.send(Err(e))),
            })
            .map_err(|e| e.to_string())?;
        let rate = match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(rate)) => rate,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                let _ = stop_tx.send(());
                return Err("the microphone did not start".into());
            }
        };
        Ok(Box::new(Take { stop: Some(stop_tx), thread: Some(thread), samples, rate }))
    }
}

/// Open the default input device and start it: each callback adds its frames, mixed to mono.
fn open(buffer: Arc<Mutex<Vec<f32>>>) -> Result<(cpal::Stream, u32), String> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or_else(|| "no microphone was found".to_string())?;
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let rate = supported.sample_rate();
    let channels = usize::from(supported.channels()).max(1);
    let cap = usize::try_from(rate).unwrap_or(48_000).saturating_mul(MAX_SECONDS);
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let on_error = |e: cpal::Error| log::warn!("microphone: {e}");
    let stream = match format {
        cpal::SampleFormat::F32 => device.build_input_stream(config, move |data: &[f32], _: &_| keep(&buffer, data, channels, cap, |s| s), on_error, None),
        cpal::SampleFormat::I16 => {
            device.build_input_stream(config, move |data: &[i16], _: &_| keep(&buffer, data, channels, cap, |s| f32::from(s) / 32_768.0), on_error, None)
        }
        cpal::SampleFormat::U16 => device.build_input_stream(
            config,
            move |data: &[u16], _: &_| keep(&buffer, data, channels, cap, |s| (f32::from(s) - 32_768.0) / 32_768.0),
            on_error,
            None,
        ),
        cpal::SampleFormat::I32 => device.build_input_stream(
            config,
            move |data: &[i32], _: &_| keep(&buffer, data, channels, cap, |s| (f64::from(s) / 2_147_483_648.0) as f32),
            on_error,
            None,
        ),
        other => return Err(format!("the microphone's sample format ({other}) is not supported")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, rate))
}

/// Add interleaved `data` to the recording as mono samples, up to `cap`.
fn keep<T: Copy>(buffer: &Mutex<Vec<f32>>, data: &[T], channels: usize, cap: usize, to_f32: impl Fn(T) -> f32) {
    let Ok(mut samples) = buffer.lock() else { return };
    for frame in data.chunks(channels) {
        if samples.len() >= cap {
            return;
        }
        let sum: f32 = frame.iter().map(|s| to_f32(*s)).sum();
        samples.push(sum / frame.len().max(1) as f32);
    }
}

/// A recording in progress: the thread holding the stream and what it has heard so far.
struct Take {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    samples: Arc<Mutex<Vec<f32>>>,
    rate: u32,
}

impl Take {
    /// Close the device and wait for its thread.
    fn close(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

impl Recording for Take {
    fn finish(mut self: Box<Self>) -> Result<Vec<u8>, String> {
        self.close();
        let samples = self.samples.lock().map(|mut s| std::mem::take(&mut *s)).map_err(|_| "the recording was lost".to_string())?;
        if samples.is_empty() {
            return Err(solidcraft_ui_text::tr("ui.dictation_nothing_recorded").to_string());
        }
        Ok(dictation::wav_16k(&samples, self.rate))
    }
}

impl Drop for Take {
    /// A recording dropped without finish() (the panel closed) turns the microphone off too.
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_mixed_to_mono_and_capped() {
        let buffer = Mutex::new(Vec::new());
        keep(&buffer, &[1.0f32, 0.0, 0.5, 0.5, -1.0, -1.0], 2, 2, |s| s);
        assert_eq!(*buffer.lock().unwrap(), vec![0.5, 0.5]);
        let ints = Mutex::new(Vec::new());
        keep(&ints, &[i16::MIN, 0], 1, 10, |s| f32::from(s) / 32_768.0);
        assert_eq!(*ints.lock().unwrap(), vec![-1.0, 0.0]);
    }
}
