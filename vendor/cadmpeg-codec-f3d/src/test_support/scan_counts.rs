// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::View;
use cadmpeg_ir::hash::digest::Sha256Digest;
use std::cell::RefCell;

thread_local! {
    static SCANS: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

pub(crate) fn record(root: View<'_>) {
    SCANS.with(|slot| {
        if let Some(scans) = slot.borrow_mut().as_mut() {
            scans.push(Sha256Digest::digest(root.window()).as_str().to_owned());
        }
    });
}

pub(crate) fn track<T>(work: impl FnOnce() -> T) -> (T, Vec<String>) {
    struct Restore(Option<Vec<String>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SCANS.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let previous = SCANS.with(|slot| slot.borrow_mut().replace(Vec::new()));
    let _restore = Restore(previous);
    let result = work();
    let scans = SCANS.with(|slot| slot.borrow_mut().take().unwrap());
    (result, scans)
}
