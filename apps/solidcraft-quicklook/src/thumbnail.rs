//! macOS Finder icons for SolidCraft designs: the picture a desktop save keeps in the design.
//!
//! A Quick Look thumbnail extension (`SolidCraft.app/Contents/PlugIns`). Finder asks it for a
//! design's icon; it reads `cache/thumbnail.png` with `solidcraft-design-preview`, which never
//! opens the model, and has Core Graphics scale the picture into the icon. A design without a
//! picture (a browser download, a design saved before its model was evaluated) keeps the generic
//! design icon. The system caches the icon until the file changes.

#[cfg(target_os = "macos")]
mod macos;

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
    use std::ptr::{self, NonNull};

    use block2::{DynBlock, RcBlock};
    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2::{ClassType, define_class};
    use objc2_core_foundation::{CFData, CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{CGColorRenderingIntent, CGContext, CGDataProvider, CGImage, CGInterpolationQuality};
    use objc2_foundation::NSError;
    use objc2_quick_look_thumbnailing::{QLFileThumbnailRequest, QLThumbnailProvider, QLThumbnailReply};
    use solidcraft_design_preview::Picture;

    use crate::macos;

    define_class!(
        // SAFETY: QLThumbnailProvider has no subclassing requirements beyond overriding
        // `provideThumbnailForFileRequest:completionHandler:`, and the class adds no ivars or Drop.
        #[unsafe(super(QLThumbnailProvider))]
        #[name = "SolidCraftThumbnailProvider"]
        pub struct ThumbnailProvider;

        impl ThumbnailProvider {
            // SAFETY: the signature is QLThumbnailProvider's for this selector.
            #[unsafe(method(provideThumbnailForFileRequest:completionHandler:))]
            fn provide(&self, request: &QLFileThumbnailRequest, handler: &DynBlock<dyn Fn(*mut QLThumbnailReply, *mut NSError)>) {
                match reply(request) {
                    Some(reply) => handler.call((Retained::as_ptr(&reply).cast_mut(), ptr::null_mut())),
                    None => {
                        let error = macos::nothing_to_show();
                        handler.call((ptr::null_mut(), Retained::as_ptr(&error).cast_mut()));
                    }
                }
            }
        }
    );

    /// The icon for the design `request` names: its picture, as large as the request allows.
    fn reply(request: &QLFileThumbnailRequest) -> Option<Retained<QLThumbnailReply>> {
        // SAFETY: plain property reads on the request the host handed us.
        let (url, largest, scale) = unsafe { (request.fileURL(), request.maximumSize(), request.scale()) };
        let picture = solidcraft_design_preview::picture(&macos::file(&url)?)?;
        let (width, height) = picture.fit(largest.width, largest.height)?;
        // The context is in pixels: the reply's size times the request's scale.
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        let pixels = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width * scale, height * scale));
        let draw = RcBlock::new(move |context: NonNull<CGContext>| Bool::new(draw(context, &picture, pixels)));
        // SAFETY: the block matches the drawing block's signature, and the reply keeps it.
        Some(unsafe { QLThumbnailReply::replyWithContextSize_drawingBlock(CGSize::new(width, height), &draw) })
    }

    /// Decode `picture` and draw it filling `rect` of `context`; false when it doesn't decode.
    fn draw(context: NonNull<CGContext>, picture: &Picture, rect: CGRect) -> bool {
        let data = CFData::from_bytes(&picture.png);
        let Some(source) = CGDataProvider::with_cf_data(Some(&data)) else { return false };
        // SAFETY: a null decode array means the image's own value ranges, as the function allows.
        let image = unsafe { CGImage::with_png_data_provider(Some(&source), ptr::null(), true, CGColorRenderingIntent::RenderingIntentDefault) };
        let Some(image) = image else { return false };
        // SAFETY: the host passes a valid bitmap context for the duration of the drawing block.
        let context = unsafe { context.as_ref() };
        CGContext::set_interpolation_quality(Some(context), CGInterpolationQuality::High);
        CGContext::draw_image(Some(context), rect, Some(&image));
        true
    }

    /// Register the class before the extension host looks it up by name.
    pub fn register() {
        ThumbnailProvider::class();
    }
}
