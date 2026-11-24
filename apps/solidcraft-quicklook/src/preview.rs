//! macOS Quick Look previews of SolidCraft designs: the model in 3D, turned with the pointer.
//!
//! A Quick Look preview extension (`SolidCraft.app/Contents/PlugIns`). Pressing Space on a design
//! in Finder shows the display meshes a save keeps in the design (`cache/preview.bin`, read with
//! `solidcraft-design-preview`), drawn with Metal ([`viewer`]): drag to turn the model, scroll or
//! pinch to zoom. A design without them shows its saved picture instead, and one with neither
//! leaves Quick Look its generic card. Nothing evaluates or tessellates the model, and the GPU
//! draws only when the view changes.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod viewer;

#[cfg(target_os = "macos")]
fn main() {
    std::process::exit(macos::run(provider::register));
}

/// The extension exists only on macOS; elsewhere the binary does nothing.
#[cfg(not(target_os = "macos"))]
fn main() {}

/// The extension's principal class (`NSExtensionPrincipalClass` in its Info.plist).
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod provider {
    use std::ptr;

    use block2::DynBlock;
    use objc2::rc::Retained;
    use objc2::{AnyThread, ClassType, MainThreadOnly, define_class};
    use objc2_app_kit::{NSAutoresizingMaskOptions, NSImage, NSImageScaling, NSImageView, NSResponder, NSView, NSViewController};
    use objc2_foundation::{NSData, NSError, NSObject, NSObjectProtocol, NSSize, NSURL};
    use objc2_quick_look_ui::QLPreviewingController;

    use crate::{macos, viewer};

    /// The preview's size before the host lays it out, in points.
    const SIZE: NSSize = NSSize::new(800.0, 600.0);

    define_class!(
        // SAFETY: NSViewController's subclasses may override `loadView`, and Quick Look's view
        // controllers implement QLPreviewingController, as this one does. No ivars, no Drop.
        #[unsafe(super(NSViewController, NSResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "SolidCraftPreviewController"]
        pub struct PreviewController;

        unsafe impl NSObjectProtocol for PreviewController {}

        // SAFETY: the method's signature is the protocol's for this selector.
        unsafe impl QLPreviewingController for PreviewController {
            #[unsafe(method(preparePreviewOfFileAtURL:completionHandler:))]
            fn prepare(&self, url: &NSURL, handler: &DynBlock<dyn Fn(*mut NSError)>) {
                if self.show(url) {
                    handler.call((ptr::null_mut(),));
                } else {
                    let error = macos::nothing_to_show();
                    handler.call((Retained::as_ptr(&error).cast_mut(),));
                }
            }
        }

        impl PreviewController {
            // SAFETY: `loadView` takes and returns nothing; there is no nib to load.
            #[unsafe(method(loadView))]
            fn load_view(&self) {
                self.setView(&NSView::new(self.mtm()));
                self.setPreferredContentSize(SIZE);
            }
        }
    );

    impl PreviewController {
        /// Put the design at `url` in the view: its model, or else its picture. False: neither.
        fn show(&self, url: &NSURL) -> bool {
            let Some(path) = macos::file(url) else { return false };
            let mtm = self.mtm();
            if let Some(model) = solidcraft_design_preview::model(&path)
                && let Some(view) = viewer::ModelView::new(mtm, model)
            {
                self.fill(&view);
                return true;
            }
            let Some(picture) = solidcraft_design_preview::picture(&path) else { return false };
            let Some(image) = NSImage::initWithData(NSImage::alloc(), &NSData::from_vec(picture.png)) else { return false };
            let view = NSImageView::imageViewWithImage(&image, mtm);
            view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
            self.fill(&view);
            true
        }

        /// Show `content` filling the controller's view and following its size. The host has
        /// already taken the controller's view when it asks for the preview, so the content goes
        /// inside that view rather than replacing it.
        fn fill(&self, content: &NSView) {
            let host = self.view();
            content.setFrame(host.bounds());
            content.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
            host.addSubview(content);
        }
    }

    /// Register the classes before the extension host looks the controller up by name.
    pub fn register() {
        PreviewController::class();
        viewer::ModelView::class();
    }
}
