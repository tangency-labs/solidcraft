//! iOS file services. UIKit owns presentation; Rust owns the imported sandbox files.
//!
//! The app denies unsafe code. This module is the narrow exception: Objective-C
//! delegate definitions, superclass initialisation and framework globals have no
//! safe Rust equivalents. UIKit objects are created and retained only on the main
//! thread; the import worker receives only a file descriptor and owned Rust data.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::future::Future;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};
use std::time::{SystemTime, UNIX_EPOCH};

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSURL};
use objc2_ui_kit::{
    UIAdaptivePresentationControllerDelegate, UIApplication, UIDocumentPickerDelegate, UIDocumentPickerViewController, UIPresentationController,
    UIViewController,
};
use objc2_uniform_type_identifiers::{UTTypeContent, UTTypeData};
use solidcraft_ui_egui::{FilePickerFuture, Services};

use crate::paths::save_path;

#[derive(Default)]
struct AnswerState {
    complete: bool,
    importing: bool,
    path: Option<String>,
    waker: Option<Waker>,
}

struct Answer {
    state: Mutex<AnswerState>,
    ctx: egui::Context,
}

impl Answer {
    fn state(&self) -> MutexGuard<'_, AnswerState> {
        // A poisoned lock must not turn a cancelled file picker into an app crash.
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn finish(&self, path: Option<String>) {
        let waker = {
            let mut state = self.state();
            if state.complete {
                return;
            }
            state.complete = true;
            state.path = path;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
        self.ctx.request_repaint();
    }

    fn cancel(&self) {
        // UIKit may dismiss its sheet while the worker persists a selected file.
        if !self.state().importing {
            self.finish(None);
        }
    }

    fn begin_import(&self) -> bool {
        let mut state = self.state();
        if state.complete || state.importing {
            return false;
        }
        state.importing = true;
        true
    }
}

struct PickerFuture(Arc<Answer>);

impl Future for PickerFuture {
    type Output = Option<String>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.0.state();
        if state.complete {
            Poll::Ready(state.path.take())
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

struct DelegateIvars {
    documents: PathBuf,
    answer: Arc<Answer>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements. There is no custom Drop,
    // and MainThreadOnly confines all UIKit access to its required thread.
    #[unsafe(super = NSObject)]
    #[name = "SolidCraftDocumentPickerDelegate"]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    struct PickerDelegate;

    // SAFETY: NSObjectProtocol imposes no additional requirements.
    unsafe impl NSObjectProtocol for PickerDelegate {}

    // SAFETY: Both selectors and their signatures match UIDocumentPickerDelegate.
    unsafe impl UIDocumentPickerDelegate for PickerDelegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn picked(&self, _picker: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
            let ivars = self.ivars();
            if !ivars.answer.begin_import() {
                return;
            }
            let source = urls.firstObject().filter(|url| url.isFileURL()).and_then(|url| url.to_file_path());
            let Some(source) = source else {
                ivars.answer.finish(None);
                return;
            };
            // Open the asCopy URL before returning to UIKit. The worker keeps
            // this descriptor, not a temporary URL or a security-scoped grant.
            let input = fs::metadata(&source).ok().filter(|metadata| metadata.is_file()).and_then(|_| File::open(&source).ok());
            let Some(input) = input else {
                ivars.answer.finish(None);
                return;
            };
            let Some(name) = source.file_name().map(|name| name.to_owned()) else {
                ivars.answer.finish(None);
                return;
            };
            let imports = ivars.documents.join("Imports");
            let answer = Arc::clone(&ivars.answer);
            // Copying large CAD files must not block UIKit's event loop.
            let started = std::thread::Builder::new().name("document-import".into()).spawn(move || {
                let path = persist_import(input, &imports, &name).ok().and_then(|path| path.into_os_string().into_string().ok());
                answer.finish(path);
            });
            if started.is_err() {
                ivars.answer.finish(None);
            }
        }

        #[unsafe(method(documentPickerWasCancelled:))]
        fn cancelled(&self, _picker: &UIDocumentPickerViewController) {
            self.ivars().answer.cancel();
        }
    }

    // SAFETY: This optional method has the protocol's exact selector/signature.
    unsafe impl UIAdaptivePresentationControllerDelegate for PickerDelegate {
        #[unsafe(method(presentationControllerDidDismiss:))]
        fn dismissed(&self, _presentation: &UIPresentationController) {
            self.ivars().answer.cancel();
        }
    }
);

impl PickerDelegate {
    fn new(mtm: MainThreadMarker, documents: PathBuf, answer: Arc<Answer>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars { documents, answer });
        // SAFETY: NSObject's init takes no arguments and returns this instance.
        unsafe { msg_send![super(this), init] }
    }
}

struct ActivePicker {
    _picker: Retained<UIDocumentPickerViewController>,
    _delegate: Retained<PickerDelegate>,
    answer: Arc<Answer>,
}

thread_local! {
    // UIKit's delegate properties are weak. Retain both objects until at least
    // the next request, never releasing a delegate from inside its own callback.
    static ACTIVE: RefCell<Option<ActivePicker>> = const { RefCell::new(None) };
}

fn presenter(mtm: MainThreadMarker) -> Option<Retained<UIViewController>> {
    let application = UIApplication::sharedApplication(mtm);
    // This winit host owns one UIWindow and does not use the UIScene lifecycle.
    #[allow(deprecated)]
    let window = application.keyWindow()?;
    let mut controller = window.rootViewController()?;
    for _ in 0..32 {
        if controller.isBeingDismissed() || controller.isBeingPresented() {
            return None;
        }
        if let Some(next) = controller.presentedViewController() {
            controller = next;
        } else {
            controller.viewIfLoaded()?.window()?;
            return Some(controller);
        }
    }
    None
}

fn open(documents: &Path, ctx: &egui::Context) -> FilePickerFuture {
    let answer = Arc::new(Answer { state: Mutex::new(AnswerState::default()), ctx: ctx.clone() });
    let future = Box::pin(PickerFuture(Arc::clone(&answer)));
    let Some(mtm) = MainThreadMarker::new() else {
        answer.finish(None);
        return future;
    };
    let Some(presenter) = presenter(mtm) else {
        answer.finish(None);
        return future;
    };
    // SAFETY: These immutable, non-null framework constants are available from
    // iOS 14, below this host's minimum deployment version.
    let types = unsafe { NSArray::from_slice(&[UTTypeData, UTTypeContent]) };
    let picker = UIDocumentPickerViewController::initForOpeningContentTypes_asCopy(UIDocumentPickerViewController::alloc(mtm), &types, true);
    picker.setAllowsMultipleSelection(false);
    let delegate = PickerDelegate::new(mtm, documents.to_owned(), Arc::clone(&answer));
    picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    let retained = ACTIVE.with(|slot| {
        let Ok(mut active) = slot.try_borrow_mut() else {
            return false;
        };
        if active.as_ref().is_some_and(|active| !active.answer.state().complete) {
            return false;
        }
        *active = Some(ActivePicker { _picker: picker.clone(), _delegate: delegate.clone(), answer: Arc::clone(&answer) });
        true
    });
    if !retained {
        answer.finish(None);
        return future;
    }
    presenter.presentViewController_animated_completion(&picker, true, None);
    if let Some(presentation) = picker.presentationController() {
        // SAFETY: The object implements this exact protocol and ACTIVE keeps it
        // alive for the lifetime of UIKit's weak delegate reference.
        unsafe { presentation.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
    }
    // UIKit refuses presentation during a transition or without a visible host.
    // Never leave the UI's picker future pending after such a refusal.
    if picker.presentingViewController().is_none() {
        answer.finish(None);
    }
    future
}

fn persist_import(input: File, imports: &Path, name: &std::ffi::OsStr) -> io::Result<PathBuf> {
    // Preserve the original basename and extension, but never replace a previous
    // import. A new private directory also isolates incomplete copies after a
    // crash; no design or previous import can be truncated by this operation.
    if !input.metadata()?.is_file() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    fs::create_dir_all(imports)?;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(io::Error::other)?.as_nanos();
    for attempt in 0..128 {
        let directory = imports.join(format!("{timestamp}-{attempt}"));
        match fs::create_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
        let partial = directory.join(".importing");
        let destination = directory.join(name);
        let result = (|| {
            let mut output = OpenOptions::new().write(true).create_new(true).open(&partial)?;
            // The engine's largest CAD input budget also bounds disk use here;
            // Read::take handles a source file that grows after it was selected.
            let maximum = solidcraft_engine::io::MAX_STEP_BYTES as u64;
            let copied = io::copy(&mut input.take(maximum + 1), &mut output)?;
            if copied > maximum {
                return Err(io::Error::from(io::ErrorKind::InvalidData));
            }
            output.sync_all()?;
            drop(output);
            fs::rename(&partial, &destination)?;
            Ok(destination)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&partial);
            let _ = fs::remove_dir(&directory);
        }
        return result;
    }
    Err(io::Error::from(io::ErrorKind::AlreadyExists))
}

pub fn services(documents: &Path, ctx: &egui::Context) -> Services {
    let open_documents = documents.to_owned();
    let file_documents = documents.to_owned();
    let save_documents = documents.to_owned();
    let open_ctx = ctx.clone();
    let file_ctx = ctx.clone();
    Services {
        pick_open: Some(Box::new(move |_| open(&open_documents, &open_ctx))),
        pick_file: Some(Box::new(move |_, _| open(&file_documents, &file_ctx))),
        pick_save: Some(Box::new(move |name, _| Box::pin(std::future::ready(save_path(&save_documents, name))))),
        ..Services::default()
    }
}
