//! macOS: files opened from Finder (double-click, Open With, a drop on the Dock icon) or with
//! `open -a SolidCraft file` (#997).
//!
//! macOS doesn't pass these on the command line: AppKit hands them to the application delegate's
//! `application:openURLs:`, and when the delegate has no such method it asks `NSDocumentController`,
//! which fails with "SolidCraft cannot open files in the … format" because SolidCraft isn't an
//! `NSDocument` app. winit 0.30's delegate doesn't implement the method, so [`install`] adds it to
//! the delegate's class when the app is about to finish launching: after winit set the delegate and
//! before AppKit delivers the files the app was launched with. The handler queues the paths and
//! brings the window forward, and wakes the UI, which opens them with [`take`] like files named
//! on the command line.
//!
//! Adding a method to another crate's Objective-C class has no safe API, hence the `unsafe_code`
//! allowance on [`install`] and [`add_open_urls_method`] (winit overrides `sendEvent:` on
//! `NSApplication` similarly).

use std::ptr::NonNull;
use std::sync::{Mutex, OnceLock, PoisonError};

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::runtime::{AnyObject, Imp, Sel};
use objc2::sel;
use objc2_app_kit::{NSApplication, NSApplicationWillFinishLaunchingNotification};
use objc2_foundation::{NSArray, NSNotification, NSNotificationCenter, NSURL};

/// Paths that arrived and haven't been opened yet.
static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The UI to wake when files arrive (unset until the window exists; the first frame opens them).
static UI: OnceLock<egui::Context> = OnceLock::new();

/// Handle open-document events from now on. Call before the event loop runs.
#[allow(unsafe_code)]
pub fn install() {
    let block = RcBlock::new(|_: NonNull<NSNotification>| add_open_urls_method());
    // SAFETY: the name is AppKit's notification constant; no object or queue filter, so the block
    // runs on the posting thread (AppKit posts it on the main thread). The block captures nothing,
    // so it is sendable. The notification center keeps the registration (and the block) for the
    // life of the app.
    unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSApplicationWillFinishLaunchingNotification),
            None,
            None,
            &block,
        );
    }
}

/// Bring `ctx`'s window forward when files arrive, including files queued during startup.
pub fn set_ui(ctx: &egui::Context) {
    // Set once; a second call would hand over the same context.
    if UI.set(ctx.clone()).is_ok() && !PENDING.lock().unwrap_or_else(PoisonError::into_inner).is_empty() {
        bring_forward(ctx);
    }
}

fn bring_forward(ctx: &egui::Context) {
    // Focus alone is ignored when AppKit still marks a window on another Space as key.
    // Showing it also orders it to the front; restore it first so focus can activate the app.
    for command in [egui::ViewportCommand::Minimized(false), egui::ViewportCommand::Visible(true), egui::ViewportCommand::Focus] {
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, command);
    }
}

/// The paths that arrived since the last call.
pub fn take() -> Vec<String> {
    std::mem::take(&mut *PENDING.lock().unwrap_or_else(PoisonError::into_inner))
}

/// `application:openURLs:` on the application delegate's class.
#[allow(unsafe_code)]
fn add_open_urls_method() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() else {
        log::warn!("no application delegate; files opened from Finder won't open");
        return;
    };
    let class = AsRef::<AnyObject>::as_ref(&*delegate).class();
    let handler: extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &NSArray<NSURL>) = open_urls;
    // SAFETY: `Imp` is the untyped form of a method implementation; the Objective-C runtime calls
    // it with the arguments the type encoding declares, which match `open_urls`: no return value
    // (`v`), the receiver (`@`), the selector (`:`), the application (`@`) and the URL array (`@`).
    let added = unsafe {
        let imp = std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &NSArray<NSURL>), Imp>(handler);
        objc2::ffi::class_addMethod(std::ptr::from_ref(class).cast_mut(), sel!(application:openURLs:), imp, c"v@:@@".as_ptr())
    };
    if !added.as_bool() {
        log::warn!("the application delegate already handles open-document events");
    }
}

/// AppKit calls this; a panic stops here instead of unwinding through AppKit's frames.
extern "C-unwind" fn open_urls(_delegate: &AnyObject, _cmd: Sel, _app: &AnyObject, urls: &NSArray<NSURL>) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| queue(urls))).is_err() {
        log::warn!("files opened from Finder were dropped after an internal error");
    }
}

/// Queue the files among `urls` and bring the window forward.
fn queue(urls: &NSArray<NSURL>) {
    // Only file URLs: macOS 27 turns an https URL's path into a file path too.
    let paths: Vec<_> = urls.iter().filter(|u| u.isFileURL()).filter_map(|u| u.to_file_path()).map(|p| p.to_string_lossy().into_owned()).collect();
    if paths.is_empty() {
        return;
    }
    PENDING.lock().unwrap_or_else(PoisonError::into_inner).extend(paths);
    if let Some(ctx) = UI.get() {
        bring_forward(ctx);
    }
}

#[cfg(test)]
mod tests {
    use objc2::runtime::NSObject;
    use objc2_foundation::NSString;

    use super::*;

    fn window_commands(ctx: &egui::Context, mut action: impl FnMut()) -> Vec<egui::ViewportCommand> {
        let mut output = ctx.run_ui(Default::default(), |_| action());
        // This test has no renderer to consume font textures.
        output.textures_delta.clear();
        output
            .viewport_output
            .remove(&egui::ViewportId::ROOT)
            .unwrap()
            .commands
            .into_iter()
            .filter(|command| !matches!(command, egui::ViewportCommand::SetTheme(_)))
            .collect()
    }

    /// Finder files wait for the UI and bring its window forward once per file event.
    #[test]
    fn opened_files_wait_for_the_ui() {
        let web = NSURL::URLWithString(&NSString::from_str("https://example.com/a.solidcraft")).unwrap();
        let urls = NSArray::from_retained_slice(&[
            NSURL::from_file_path("/tmp/a b.solidcraft").unwrap(),
            web.clone(),
            NSURL::from_file_path("/tmp/c.step").unwrap(),
        ]);
        let any = NSObject::new();
        open_urls(&any, sel!(application:openURLs:), &any, &urls);

        let ctx = egui::Context::default();
        let bring_forward = [egui::ViewportCommand::Minimized(false), egui::ViewportCommand::Visible(true), egui::ViewportCommand::Focus];
        assert_eq!(window_commands(&ctx, || set_ui(&ctx)), bring_forward, "startup files must bring the window forward");

        // Invalid or empty events must not refocus the window, even with files still pending.
        let web_only = NSArray::from_retained_slice(&[web]);
        let empty = NSArray::from_retained_slice(&[]);
        assert!(
            window_commands(&ctx, || {
                open_urls(&any, sel!(application:openURLs:), &any, &web_only);
                open_urls(&any, sel!(application:openURLs:), &any, &empty);
                set_ui(&ctx);
            })
            .is_empty()
        );
        assert!(
            window_commands(&ctx, || {
                assert_eq!(take(), ["/tmp/a b.solidcraft", "/tmp/c.step"]);
                assert!(take().is_empty());
            })
            .is_empty()
        );

        assert_eq!(window_commands(&ctx, || open_urls(&any, sel!(application:openURLs:), &any, &urls)), bring_forward);
        assert_eq!(take(), ["/tmp/a b.solidcraft", "/tmp/c.step"]);
        assert!(window_commands(&ctx, || set_ui(&ctx)).is_empty(), "idle frames must leave focus alone");
        assert!(take().is_empty());
    }
}
