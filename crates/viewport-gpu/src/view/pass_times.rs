//! The GPU time of the view's own render passes, for the frame-rate readout's details: what the
//! CPU times there (UI, scene build) leave out. Each pass the renderer records asks for a begin and
//! an end timestamp; the frame's timestamps are resolved into a buffer that is read back a frame or
//! two later, so the window never waits for the GPU. Only on a device opened with
//! [`wgpu::Features::TIMESTAMP_QUERY`], and only while the readout asks for it
//! ([`super::DrawStats::time_passes`]).

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

/// Passes timed in one frame at most; later passes of a frame are not timed.
pub const MAX_PASSES: usize = 16;
/// Bytes of one timestamp.
const STAMP: u64 = 8;
/// Frames whose timestamps can wait to be read back at once.
const RING: usize = 3;
/// Frames timed once more, at most, each time timing is asked for: see [`Again`].
const AGAIN: u8 = 2;

/// A read-back buffer's state.
const FREE: u8 = 0;
/// Its copy was recorded this frame; it is mapped once that frame was submitted.
const COPIED: u8 = 1;
const MAPPING: u8 = 2;
const MAPPED: u8 = 3;
/// The map failed; the buffer is used again.
const FAILED: u8 = 4;

/// The GPU times of one frame's timed passes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PassTimes {
    /// From the first timed pass's start to the last one's end, in microseconds: passes overlap on
    /// a tiled GPU (one pass's vertices with the previous one's fragments), so this is less than
    /// their sum.
    pub span_us: u32,
    /// The longest pass and its time, in microseconds (its share of the span, as in [`PassTimes::each`]).
    pub slowest: &'static str,
    pub slowest_us: u32,
    /// Passes timed (with valid timestamps).
    pub passes: u32,
    /// Every pass asked to be timed, in order: its label and its share of the span in microseconds
    /// (from the end of the passes before it, or its own start if later, to its end: on a tiled GPU
    /// a pass starts before the one before it has finished), `None` where the device gave no valid
    /// timestamps for it (a driver may merge a pass into the one before it). The first `asked`
    /// entries are used.
    pub each: [(&'static str, Option<u32>); MAX_PASSES],
    pub asked: u32,
}

impl PassTimes {
    /// The passes asked to be timed, in order, with their times ([`PassTimes::each`]).
    pub fn passes(&self) -> &[(&'static str, Option<u32>)] {
        self.each.get(..self.asked as usize).unwrap_or_default()
    }
}

/// Whether to time one more frame after a frame whose timestamps were read back without one valid
/// pass. A still view times a single frame when timing is asked for, and a new query set's first
/// frame reads back as zeros on Apple's GPUs (seen on an M6): without another frame the times would
/// wait for the view to change. At most [`AGAIN`] frames each time timing is asked for, so a device
/// that never gives a timestamp draws a still view a few times, not for ever.
#[derive(Default)]
struct Again {
    asked: bool,
    left: u8,
}

impl Again {
    /// Start a frame: `on` while timing is asked for. Asking anew allows [`AGAIN`] more frames.
    fn frame(&mut self, on: bool) {
        if on && !self.asked {
            self.left = AGAIN;
        }
        self.asked = on;
    }

    /// A frame's read-back gave no times, and `known` says whether an earlier frame's are there:
    /// whether to time one more frame.
    fn after_empty(&mut self, known: bool) -> bool {
        let again = self.asked && !known && self.left > 0;
        if again {
            self.left -= 1;
        }
        again
    }
}

struct Readback {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    labels: Vec<&'static str>,
}

/// Timestamps for the view's passes ([`PassTimer::writes`]), read back without waiting.
pub(crate) struct PassTimer {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    ring: Vec<Readback>,
    /// This frame's timed passes, in the order their timestamps were asked for; empty while
    /// nothing is timed.
    labels: Mutex<Vec<&'static str>>,
    on: bool,
    /// Nanoseconds per timestamp tick.
    period: f64,
    /// The last frame read back.
    pub last: Option<PassTimes>,
    again: Again,
}

impl PassTimer {
    /// A timer for `device`, or `None` where it has no timestamp queries.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<PassTimer> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let count = 2 * MAX_PASSES as u32;
        let bytes = u64::from(count) * STAMP;
        let set = device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("sc_pass_times"), ty: wgpu::QueryType::Timestamp, count });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_pass_times_resolve"),
            size: bytes,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let ring = (0..RING)
            .map(|_| Readback {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("sc_pass_times_read"),
                    size: bytes,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU8::new(FREE)),
                labels: Vec::new(),
            })
            .collect();
        Some(PassTimer {
            set,
            resolve,
            ring,
            labels: Mutex::new(Vec::new()),
            on: false,
            period: f64::from(queue.get_timestamp_period()),
            last: None,
            again: Again::default(),
        })
    }

    /// Start a frame: read back what earlier frames' timestamps finished, map the buffers the
    /// last frame filled (it has been submitted since), and time this frame's passes when `on`.
    /// Returns whether this frame should be drawn, not shown from a picture already drawn: a
    /// frame's timestamps came back without times and none are known yet ([`Again`]).
    pub fn begin(&mut self, device: &wgpu::Device, on: bool) -> bool {
        // Runs the map callbacks of finished copies; never waits.
        let _ = device.poll(wgpu::PollType::Poll);
        self.again.frame(on);
        let mut empty = false;
        for r in &mut self.ring {
            match r.state.load(Ordering::Acquire) {
                COPIED => {
                    r.state.store(MAPPING, Ordering::Release);
                    let state = r.state.clone();
                    r.buffer.slice(..).map_async(wgpu::MapMode::Read, move |done| {
                        state.store(if done.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
                    });
                }
                MAPPED => {
                    // A range that cannot be read gives no times this frame.
                    let stamps: Vec<u64> = match r.buffer.slice(..).get_mapped_range() {
                        Ok(view) => view.as_chunks::<{ STAMP as usize }>().0.iter().map(|b| u64::from_le_bytes(*b)).collect(),
                        Err(_) => Vec::new(),
                    };
                    r.buffer.unmap();
                    match times(&r.labels, &stamps, self.period) {
                        Some(times) => self.last = Some(times),
                        None => empty = true,
                    }
                    r.state.store(FREE, Ordering::Release);
                }
                FAILED => {
                    empty = true;
                    r.state.store(FREE, Ordering::Release);
                }
                _ => {}
            }
        }
        // Nothing is timed with no buffer free to read it back.
        self.on = on && self.ring.iter().any(|r| r.state.load(Ordering::Acquire) == FREE);
        if !on {
            self.last = None;
        }
        if let Ok(mut labels) = self.labels.lock() {
            labels.clear();
        }
        empty && self.again.after_empty(self.last.is_some())
    }

    /// The timestamp writes of the next pass, `label`; `None` while nothing is timed or the
    /// frame's slots are used.
    pub fn writes(&self, label: &'static str) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        if !self.on {
            return None;
        }
        let mut labels = self.labels.lock().ok()?;
        let i = u32::try_from(labels.len()).ok().filter(|&i| (i as usize) < MAX_PASSES)?;
        labels.push(label);
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(2 * i),
            end_of_pass_write_index: Some(2 * i + 1),
        })
    }

    /// End the frame: copy its timestamps into a free read-back buffer, mapped next frame.
    pub fn finish(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let labels = match self.labels.lock() {
            Ok(mut labels) => std::mem::take(&mut *labels),
            Err(_) => return,
        };
        let Some(r) = self.ring.iter_mut().find(|r| r.state.load(Ordering::Acquire) == FREE) else { return };
        let Ok(n) = u32::try_from(labels.len()) else { return };
        if n == 0 {
            return;
        }
        encoder.resolve_query_set(&self.set, 0..2 * n, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &r.buffer, 0, u64::from(2 * n) * STAMP);
        r.labels = labels;
        r.state.store(COPIED, Ordering::Release);
    }
}

/// The pass times of `labels` from their `stamps` (begin and end of each, in ticks of `period`
/// nanoseconds); passes whose timestamps are missing or out of order are left out.
pub(crate) fn times(labels: &[&'static str], stamps: &[u64], period: f64) -> Option<PassTimes> {
    let us = |ticks: u64| ((ticks as f64) * period / 1000.0).round().clamp(0.0, f64::from(u32::MAX)) as u32;
    let mut first = u64::MAX;
    let mut last = 0;
    let mut out = PassTimes::default();
    for (i, (label, &[begin, end])) in labels.iter().zip(stamps.as_chunks::<2>().0).enumerate().take(MAX_PASSES) {
        if let Some(entry) = out.each.get_mut(i) {
            *entry = (label, None);
            out.asked += 1;
        }
        if begin == 0 || end < begin {
            continue;
        }
        // The pass's own share: from where the passes before it ended (or its own start, if later)
        // to its end. A tiled GPU starts a pass's vertices while the pass before it still draws its
        // fragments, so the pass's start stamp comes early and its plain duration counts the wait
        // for that pass too (Apple's: a 1 ms pass after a 40 ms one read about 40 ms).
        let t = us(end.saturating_sub(begin.max(last)));
        if let Some(entry) = out.each.get_mut(i) {
            entry.1 = Some(t);
        }
        first = first.min(begin);
        last = last.max(end);
        out.passes += 1;
        if t >= out.slowest_us {
            (out.slowest, out.slowest_us) = (label, t);
        }
    }
    (out.passes > 0).then(|| PassTimes { span_us: us(last - first), ..out })
}

#[cfg(test)]
mod tests {
    use super::{AGAIN, Again, times};

    /// A still view times one frame when timing is asked for; when that frame's timestamps come
    /// back empty (a new query set's first frame on Apple's GPUs), another frame is timed, a few
    /// times at most, and never once times are known or timing is no longer asked for.
    #[test]
    fn a_frame_read_back_without_times_is_timed_again_a_few_times_at_most() {
        let mut again = Again::default();
        assert!(!again.after_empty(false), "nothing is timed again before timing is asked for");
        again.frame(true);
        assert!(!again.after_empty(true), "an empty frame is left alone while earlier times are known");
        for _ in 0..AGAIN {
            again.frame(true);
            assert!(again.after_empty(false), "an empty frame with no times known is timed again");
        }
        again.frame(true);
        assert!(!again.after_empty(false), "a device that gives no timestamps is not drawn for ever");
        again.frame(false);
        assert!(!again.after_empty(false), "nothing is timed again once timing is no longer asked for");
        again.frame(true);
        assert!(again.after_empty(false), "asking anew allows more frames");
    }

    #[test]
    fn the_span_runs_from_the_first_start_to_the_last_end_and_names_the_slowest_pass() {
        // Ticks of 1 ns: the second pass starts before the first ends (a tiled GPU's overlap).
        let t = times(&["sc_ao_prepass", "sc_ssaa", "sc_oit"], &[1_000, 2_000, 1_500, 21_500, 22_000, 24_500], 1.0);
        let t = t.expect("three passes timed");
        assert_eq!((t.span_us, t.slowest, t.slowest_us, t.passes), (24, "sc_ssaa", 20, 3));
        assert_eq!(t.passes(), [("sc_ao_prepass", Some(1)), ("sc_ssaa", Some(20)), ("sc_oit", Some(3))]);
    }

    /// Apple's tiled GPU starts a pass's vertex work while the pass before it still draws: each
    /// later pass's own stamps then span the long pass too. Each pass is given only its own part.
    #[test]
    fn a_pass_started_early_behind_a_long_one_is_given_only_its_own_time() {
        // A 40 ms pass, then a pass and a composite whose vertices start 1 ms into it and that end
        // 1 ms and 0.1 ms after it (in nanoseconds, ticks of 1 ns).
        let stamps = [1_000_000, 41_000_000, 2_000_000, 42_000_000, 3_000_000, 42_100_000];
        let t = times(&["sc_ssaa", "sc_oit", "sc_oit_composite"], &stamps, 1.0).expect("three passes timed");
        assert_eq!(t.passes(), [("sc_ssaa", Some(40_000)), ("sc_oit", Some(1_000)), ("sc_oit_composite", Some(100))]);
        assert_eq!((t.span_us, t.slowest, t.slowest_us), (41_100, "sc_ssaa", 40_000));
    }

    #[test]
    fn missing_or_reversed_timestamps_are_left_out() {
        assert_eq!(times(&["a", "b"], &[0, 5_000, 9_000, 8_000], 1.0), None);
        let t = times(&["a", "b"], &[0, 5_000, 4_000, 10_000], 2.0).expect("one pass timed");
        assert_eq!((t.span_us, t.slowest, t.passes), (12, "b", 1));
        assert_eq!(t.passes(), [("a", None), ("b", Some(12))], "a pass without valid timestamps is still listed");
    }

    #[test]
    fn more_labels_than_timestamps_time_only_the_pairs_there_are() {
        let t = times(&["a", "b"], &[1_000, 3_000], 1.0).expect("one pass timed");
        assert_eq!((t.slowest, t.slowest_us, t.passes), ("a", 2, 1));
    }
}
