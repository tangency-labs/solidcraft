//! GPU errors are not crashes. `capture_errors` returns an error made under it, and an error no
//! scope catches at all is reported by the device's handler
//! (`solidcraft_viewport_egui::log_uncaptured_errors`) instead of panicking (#5486): on the
//! window's device that covers a surface the driver refuses to configure (D3D12 "Failed to wait
//! for GPU to come idle before reconfiguring the Surface", #1935), which stock egui-wgpu configures
//! without an error scope.

use egui_wgpu::wgpu;
use solidcraft_viewport_gpu::capture_errors;

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

/// An error no scope caught on the window's device used to reach wgpu's default handler, which
/// panics ("wgpu error: Out of Memory", #5486). With the handler installed it is reported and
/// the device goes on working.
#[test]
fn an_uncaught_gpu_error_is_not_a_panic() {
    let Some((_, device, queue)) = crate::gpu::offscreen_own_device() else { return };
    solidcraft_viewport_egui::log_uncaptured_errors(&device, crate::gpu::report_gpu_error);
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
