//! Whether a device was lost, and GPU errors caught instead of reaching the device's handler.
//! Both used to live in a patched egui-wgpu; stock egui-wgpu has neither.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether a device was lost (the driver reset it, Windows' TDR among others, #4172).
///
/// A lost device can't be used again. wgpu learns of the loss when a call into the driver fails,
/// and the device's lost callback then marks this ([`DeviceLost::watch`]).
#[derive(Clone, Debug, Default)]
pub struct DeviceLost {
    lost: Arc<AtomicBool>,
    reason: Arc<std::sync::Mutex<Option<String>>>,
}

impl DeviceLost {
    /// Watch `device`: its lost callback marks the loss. A device has one lost callback, so this
    /// replaces any set before.
    pub fn watch(device: &wgpu::Device) -> Self {
        let lost = Self::default();
        let seen = lost.clone();
        device.set_device_lost_callback(move |reason, message| {
            seen.mark(&format!("{reason:?}: {message}"));
        });
        lost
    }

    /// Mark the device lost, for `reason` (the first reason is kept).
    pub fn mark(&self, reason: &str) {
        if let Ok(mut kept) = self.reason.lock() {
            kept.get_or_insert_with(|| reason.to_owned());
        }
        self.lost.store(true, Ordering::Release);
    }

    /// The device was lost.
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    /// Why the device was lost, as wgpu said it.
    pub fn reason(&self) -> Option<String> {
        self.reason.lock().ok().and_then(|r| r.clone())
    }

    /// The flag [`DeviceLost::is_lost`] reads, for others drawing on the same device.
    pub fn flag(&self) -> Arc<AtomicBool> {
        self.lost.clone()
    }
}

/// Run `f` with `device`'s errors captured, and the first error it caused. Uncaptured, an error
/// goes to the device's handler. Native error scopes resolve at once; on the web the scope still
/// keeps the error from the handler.
pub fn capture_errors<T>(device: &wgpu::Device, f: impl FnOnce() -> T) -> (T, Option<wgpu::Error>) {
    let scopes =
        [wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal, wgpu::ErrorFilter::Validation].map(|filter| device.push_error_scope(filter));
    let value = f();
    let mut failed = None;
    for scope in scopes.into_iter().rev() {
        let mut popped = std::pin::pin!(scope.pop());
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        if let std::task::Poll::Ready(Some(error)) = std::future::Future::poll(popped.as_mut(), &mut cx) {
            failed.get_or_insert(error);
        }
    }
    (value, failed)
}

#[cfg(test)]
mod device_lost_tests {
    /// A loss is marked once, with the first reason, and every clone (and the shared flag) sees it.
    #[test]
    fn a_marked_loss_is_seen_by_every_clone() {
        let lost = super::DeviceLost::default();
        let painter = lost.clone();
        let flag = lost.flag();
        assert!(!painter.is_lost() && lost.reason().is_none());
        lost.mark("Unknown: Device lost");
        lost.mark("Destroyed: ");
        assert!(painter.is_lost());
        assert!(flag.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(painter.reason().as_deref(), Some("Unknown: Device lost"));
    }
}
