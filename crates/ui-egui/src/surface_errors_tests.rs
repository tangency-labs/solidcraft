//! A surface the driver fails to reconfigure was a crash (#1935): on D3D12, `Surface::configure`
//! reported "Failed to wait for GPU to come idle before reconfiguring the Surface", and the
//! device's default uncaptured-error handler panicked on it. The vendored egui-wgpu configures
//! its surfaces under `capture_errors`, logs the error and tries again once the retry is due. A
//! driver's refusal can't be provoked here, so these check the capture on a real device with an
//! error the validation always reports, and the retry policy with a stand-in clock and device.
//! An error no scope catches at all is reported by the device's handler
//! (`gpu::log_uncaptured_errors`) instead of panicking (#5486). A configure that meets a lost
//! device isn't taken as configured, and an acquire wgpu refuses configures the surface again
//! (#7007).

use egui_wgpu::{
    Attempt, ConfigureError, ConfigureRetry, DeviceLost, SURFACE_RECREATE_AFTER, SurfaceErrorAction, WgpuConfiguration, capture_errors,
    configure_captured, wgpu,
};
use std::time::Duration;
use web_time::Instant;

#[test]
fn an_error_under_capture_is_returned_not_a_panic() {
    let Some((_, device, _)) = crate::gpu::offscreen_device() else { return };
    // Mapped at creation with a size that isn't a multiple of 4: a validation error.
    let (_, error) = capture_errors(&device, || {
        device.create_buffer(&wgpu::BufferDescriptor { label: Some("bad"), size: 3, usage: wgpu::BufferUsages::MAP_WRITE, mapped_at_creation: true })
    });
    assert!(error.is_some_and(|e| matches!(e, wgpu::Error::Validation { .. })), "the validation error is captured");
    // The device goes on working, and a clean call reports nothing.
    let (buffer, error) = capture_errors(&device, || {
        device.create_buffer(&wgpu::BufferDescriptor { label: Some("good"), size: 4, usage: wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
    });
    assert!(error.is_none(), "{error:?}");
    assert_eq!(buffer.size(), 4);
}

/// A driver reset found by a surface configure (D3D12 waits for the present queue after wgpu has
/// dropped the surface's old configuration) reports nothing: wgpu passes device-loss errors to no
/// error scope. The configure was taken, the next acquire met a surface without a configuration,
/// and the device's default handler panicked: "Surface is not configured for presentation"
/// (#7007). `destroy` loses the device as a driver reset does; wgpu calls the lost callback once
/// it has polled.
#[test]
fn a_configure_that_meets_a_lost_device_is_not_taken() {
    // The queue is kept: wgpu calls a destroyed device's lost callback only while it has one.
    let Some((_, device, _queue)) = crate::gpu::offscreen_device() else { return };
    let lost = DeviceLost::watch(&device);
    let bad = wgpu::BufferDescriptor { label: Some("bad"), size: 3, usage: wgpu::BufferUsages::MAP_WRITE, mapped_at_creation: true };
    // On a working device a clean configure is taken and a refused one returns its error.
    assert!(configure_captured(&device, &lost, || {}).is_ok());
    let refused = configure_captured(&device, &lost, || {
        let _ = device.create_buffer(&bad);
    });
    assert!(matches!(refused, Err(ConfigureError::Refused(wgpu::Error::Validation { .. }))), "{refused:?}");

    // The device is lost during the configure, which goes on and fails quietly.
    let met_loss = configure_captured(&device, &lost, || {
        device.destroy();
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let _ = device.create_buffer(&bad);
    });
    assert!(matches!(met_loss, Err(ConfigureError::DeviceLost)), "{met_loss:?}");
    assert!(met_loss.is_err_and(|e| e.to_string() == "the graphics device was lost"));
    // Why the scope alone can't tell: on the lost device the same invalid call reports nothing.
    let (_, error) = capture_errors(&device, || device.create_buffer(&bad));
    assert!(error.is_none(), "{error:?}");
}

/// wgpu hands back `CurrentSurfaceTexture::Validation` for an acquire whose error was captured:
/// most often a surface without a configuration. Skipping the frame, as before, met the same
/// error on every later frame (#7007); the surface is configured again instead. The other
/// statuses keep their actions.
#[test]
fn an_acquire_wgpu_refuses_configures_the_surface_again() {
    let status = WgpuConfiguration::default().on_surface_status;
    let action = |s: wgpu::CurrentSurfaceTexture| match status(&s) {
        SurfaceErrorAction::SkipFrame => "skip",
        SurfaceErrorAction::Reconfigure => "reconfigure",
        SurfaceErrorAction::RecreateSurface => "recreate",
    };
    assert_eq!(action(wgpu::CurrentSurfaceTexture::Validation), "reconfigure");
    assert_eq!(action(wgpu::CurrentSurfaceTexture::Outdated), "reconfigure");
    assert_eq!(action(wgpu::CurrentSurfaceTexture::Lost), "recreate");
    assert_eq!(action(wgpu::CurrentSurfaceTexture::Occluded), "skip");
    assert_eq!(action(wgpu::CurrentSurfaceTexture::Timeout), "skip");
}

/// An error no scope caught on the window's device used to reach wgpu's default handler, which
/// panics ("wgpu error: Out of Memory", #5486). With the handler installed it is reported and
/// the device goes on working.
#[test]
fn an_uncaught_gpu_error_is_not_a_panic() {
    let Some((_, device, queue)) = crate::gpu::offscreen_device() else { return };
    crate::gpu::log_uncaptured_errors(&device);
    // No error scope around it: mapped at creation with a size that isn't a multiple of 4.
    let _bad =
        device.create_buffer(&wgpu::BufferDescriptor { label: Some("bad"), size: 3, usage: wgpu::BufferUsages::MAP_WRITE, mapped_at_creation: true });
    let (buffer, error) = capture_errors(&device, || {
        device.create_buffer(&wgpu::BufferDescriptor { label: Some("good"), size: 4, usage: wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
    });
    assert!(error.is_none(), "{error:?}");
    queue.submit([]);
    assert_eq!(buffer.size(), 4);
}

/// A surface that keeps refusing its configuration waits 16 ms before the next try, doubling with
/// each refusal in a row up to 1 s, and every few refusals the surface is recreated (asked in
/// review of #1966). Only the first refusal of the streak is worth a warning.
#[test]
fn refused_configures_back_off_and_recreate_the_surface() {
    let start = Instant::now();
    let mut retry = ConfigureRetry::default();
    let plans: Vec<_> = (0..9).map(|_| retry.refused(start)).collect();
    let delays: Vec<u128> = plans.iter().map(|plan| plan.delay.as_millis()).collect();
    assert_eq!(delays, [16, 32, 64, 128, 256, 512, 1000, 1000, 1000], "capped at a second");
    assert_eq!(plans.iter().map(|plan| plan.failures).collect::<Vec<_>>(), (1..=9).collect::<Vec<_>>());
    assert_eq!(plans.iter().filter(|plan| plan.first).count(), 1);
    assert!(plans.first().is_some_and(|plan| plan.first));
    let recreate: Vec<u32> = plans.iter().filter(|plan| plan.recreate).map(|plan| plan.failures).collect();
    assert_eq!(recreate, [SURFACE_RECREATE_AFTER, 2 * SURFACE_RECREATE_AFTER, 3 * SURFACE_RECREATE_AFTER]);
    // The last refusal's try is due a second later.
    assert_eq!(retry.wait(start), Some(Duration::from_secs(1)));
    assert!(retry.is_due(start + Duration::from_secs(1)));
}

/// The review of #1966: a repaint that comes before the retry is due (pointer input, an
/// animation, another UI update) must not configure or recreate the surface. Here the window
/// repaints every frame at 60 Hz for 10 s on a device that refuses every configure, and the
/// frames follow `paint_and_update_textures`: recreate when one is pending and due (which
/// `recreate_surface` marks done), then configure.
#[test]
fn repaints_before_the_retry_is_due_neither_configure_nor_recreate() {
    let start = Instant::now();
    // Just after a refusal, each early repaint is told how long is left and tries nothing.
    let mut retry = ConfigureRetry::default();
    retry.refused(start);
    for ms in 0..16 {
        let mut tried = false;
        let attempt = retry.attempt(start + Duration::from_millis(ms), || {
            tried = true;
            Ok::<(), ()>(())
        });
        assert!(matches!(attempt, Attempt::Waiting(left) if left == Duration::from_millis(16 - ms)), "{ms} ms: {attempt:?}");
        assert!(!tried, "{ms} ms: configured before the retry was due");
    }
    assert_eq!(retry.failures(), 1, "waiting doesn't count as a refusal");

    let frame = Duration::from_micros(16_667);
    let frames = 600;
    let mut retry = ConfigureRetry::default();
    let (mut configures, mut recreates) = (0u32, 0u32);
    let mut refusals: Vec<(Instant, Duration)> = Vec::new();
    for n in 0..frames {
        let now = start + frame * n;
        if retry.recreate_due(now) {
            assert!(retry.is_due(now), "recreated before the retry was due");
            recreates += 1;
            retry.recreated();
        } else if retry.recreate_pending() {
            assert!(!retry.is_due(now), "a due recreate was left pending");
        }
        let attempt = retry.attempt(now, || {
            configures += 1;
            Err::<(), _>("refused")
        });
        match attempt {
            Attempt::Refused(plan, _) => {
                assert_eq!(plan.recreate, retry.recreate_pending(), "{plan:?}");
                refusals.push((now, plan.delay));
            }
            Attempt::Waiting(left) => assert!(left <= Duration::from_secs(1), "{left:?}"),
            Attempt::Took(_) => panic!("the device refuses every configure"),
        }
    }
    // The tries ramp up through 16 ms .. 512 ms (7 tries in the first second), then one a second:
    // 15 configures in 600 frames, not one a frame.
    assert_eq!(configures, 15, "{refusals:?}");
    assert_eq!(refusals.len(), 15);
    // No try comes before the delay the previous refusal set.
    for pair in refusals.windows(2) {
        let [(at, delay), (next, _)] = pair else { continue };
        assert!(next.duration_since(*at) >= *delay, "{pair:?}");
    }
    // A recreate before the try after every third refusal (the 3rd, 6th, 9th and 12th; the
    // 15th's try falls after the 10 s), not every third frame.
    assert_eq!(recreates, (configures - 1) / SURFACE_RECREATE_AFTER);
    assert_eq!(recreates, 4);
    assert!(retry.recreate_pending(), "the 15th refusal asked for a recreate, still waiting for its retry");
}

/// A configure the device takes ends the streak: the next refusal is a first one again, warned
/// about and retried after 16 ms. A first configure is never held back.
#[test]
fn a_configure_the_device_takes_ends_the_streak() {
    let start = Instant::now();
    let mut retry = ConfigureRetry::default();
    assert!(retry.is_due(start), "nothing refused yet");
    for _ in 0..4 {
        retry.refused(start);
    }
    let due = start + Duration::from_millis(128);
    assert!(!retry.is_due(due - Duration::from_millis(1)));
    let attempt = retry.attempt(due, || Ok::<(), ()>(()));
    assert!(matches!(attempt, Attempt::Took(4)), "{attempt:?}");
    assert_eq!(retry, ConfigureRetry::default());
    assert_eq!(retry.failures(), 0);
    assert_eq!(retry.wait(due), None, "no deadline left");
    let plan = retry.refused(due);
    assert!(plan.first && plan.failures == 1 && !plan.recreate, "{plan:?}");
    assert_eq!(plan.delay, Duration::from_millis(16));
    assert_eq!(retry.wait(due), Some(Duration::from_millis(16)));
    // `took` alone does the same, and reports an empty streak as 0.
    assert_eq!(retry.took(), 1);
    assert_eq!(retry.took(), 0);
}

/// A configure the device takes after the refusal that asked for a recreate (a resize at the
/// retry's deadline, before the next frame) ends the streak with its recreate: the surface took
/// its configuration, so the next frame doesn't recreate it.
#[test]
fn a_configure_taken_cancels_the_pending_recreate() {
    let start = Instant::now();
    let mut retry = ConfigureRetry::default();
    let plans: Vec<_> = (0..SURFACE_RECREATE_AFTER).map(|_| retry.refused(start)).collect();
    assert!(plans.last().is_some_and(|plan| plan.recreate));
    let due = start + Duration::from_millis(64);
    assert!(retry.recreate_pending());
    assert!(!retry.recreate_due(due - Duration::from_millis(1)), "not before the deadline");
    assert!(retry.recreate_due(due));
    let attempt = retry.attempt(due, || Ok::<(), ()>(()));
    assert!(matches!(attempt, Attempt::Took(n) if n == SURFACE_RECREATE_AFTER), "{attempt:?}");
    assert!(!retry.recreate_pending());
    assert!(!retry.recreate_due(due + Duration::from_secs(10)));
    assert_eq!(retry, ConfigureRetry::default());
}

/// `recreate_surface` hands the old surface's retry to the new one with the recreate marked
/// done: the deadline still holds, the new surface doesn't recreate itself again, and the streak
/// goes on rather than starting again.
#[test]
fn the_retry_survives_a_recreate() {
    let start = Instant::now();
    let mut old = ConfigureRetry::default();
    let plans: Vec<_> = (0..SURFACE_RECREATE_AFTER).map(|_| old.refused(start)).collect();
    assert!(plans.last().is_some_and(|plan| plan.recreate));
    let mut carried = old;
    carried.recreated();
    assert!(old.recreate_pending() && !carried.recreate_pending());
    assert!(!carried.recreate_due(start + Duration::from_secs(10)));
    assert_eq!(carried.failures(), old.failures());
    let soon = start + Duration::from_millis(10);
    assert_eq!(carried.wait(soon), old.wait(soon));
    assert_eq!(carried.wait(soon), Some(Duration::from_millis(54)));
    let next = carried.refused(start + Duration::from_millis(64));
    assert!(!next.first, "{next:?}");
    assert_eq!((next.failures, next.delay), (SURFACE_RECREATE_AFTER + 1, Duration::from_millis(128)));
}
