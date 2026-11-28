//! The 3D view of a Quick Look preview: a design's display meshes drawn with Metal.
//!
//! [`ModelView`] is a layer-hosting view with a `CAMetalLayer`. It uploads the model's triangle
//! corners once, then draws a frame only when something changes: the view's size or scale, a
//! drag (the model turns as a ball under the pointer, about every axis), a Shift-drag, right-drag
//! or middle-drag (pan), a scroll or a pinch (zoom), or an Option-double-click or middle
//! double-click (reset to the starting view, leaving plain left double-click to Quick Look's own
//! open-in-app action). Between those the GPU does nothing. The shading is a
//! light from the viewer, as the app's own; the background is Quick Look's own (the layer is
//! clear where there is no model).
//!
//! Subclassing NSView and calling Metal and AppKit have no safe API for the parts used here
//! (overriding event methods, calling the superclass, raw vertex bytes), hence the `unsafe_code`
//! allowance on this module.
#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSClickGestureRecognizer, NSEvent, NSEventModifierFlags, NSGestureRecognizer, NSGestureRecognizerDelegate, NSResponder, NSView};
use objc2_core_foundation::CGSize;
use objc2_foundation::{NSObject, NSObjectProtocol, NSRect, NSSize, NSString};
use objc2_metal::{
    MTLBuffer, MTLClearColor, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue, MTLCompareFunction, MTLCreateSystemDefaultDevice,
    MTLDepthStencilDescriptor, MTLDepthStencilState, MTLDevice, MTLLibrary, MTLLoadAction, MTLPixelFormat, MTLPrimitiveType, MTLRenderCommandEncoder,
    MTLRenderPassDescriptor, MTLRenderPipelineDescriptor, MTLRenderPipelineState, MTLResourceOptions, MTLStorageMode, MTLStoreAction, MTLTexture,
    MTLTextureDescriptor, MTLTextureUsage,
};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};
use solidcraft_design_preview::{Grab, Matrices, Model, Orbit, Pan, Vertex};

/// The shaders: corners in, a light from the viewer (with a little ambient and a soft highlight)
/// out. Normals face the viewer whichever way a triangle winds, so nothing is culled.
const SHADERS: &str = r"
#include <metal_stdlib>
using namespace metal;
struct Corner { packed_float3 position; packed_float3 normal; packed_float3 color; };
struct Matrices { float4x4 clip; float4x4 view; };
struct Fragment { float4 position [[position]]; float3 normal; float3 color; };
vertex Fragment corner(uint id [[vertex_id]], const device Corner* corners [[buffer(0)]], constant Matrices& m [[buffer(1)]]) {
    Corner c = corners[id];
    Fragment f;
    f.position = m.clip * float4(float3(c.position), 1.0);
    f.normal = (m.view * float4(float3(c.normal), 0.0)).xyz;
    f.color = float3(c.color);
    return f;
}
fragment float4 shade(Fragment f [[stage_in]]) {
    float3 n = normalize(f.normal);
    if (n.z < 0.0) { n = -n; }
    float3 light = normalize(float3(-0.3, 0.4, 1.0));
    float diffuse = max(dot(n, light), 0.0);
    float highlight = pow(max(dot(n, normalize(light + float3(0.0, 0.0, 1.0))), 0.0), 48.0) * 0.18;
    return float4(f.color * (0.28 + 0.72 * diffuse) + highlight, 1.0);
}
";

const COLOR_FORMAT: MTLPixelFormat = MTLPixelFormat::BGRA8Unorm;
const DEPTH_FORMAT: MTLPixelFormat = MTLPixelFormat::Depth32Float;

/// What the view draws with: made once, when the view is made.
struct Gpu {
    layer: Retained<CAMetalLayer>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    pipeline: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    depth_test: Retained<ProtocolObject<dyn MTLDepthStencilState>>,
    corners: Retained<ProtocolObject<dyn MTLBuffer>>,
    count: usize,
    /// The depth buffer, remade when the drawable's size changes.
    depth: RefCell<Option<Retained<ProtocolObject<dyn MTLTexture>>>>,
}

/// The drag in progress, if any.
#[derive(Clone, Copy)]
enum Drag {
    None,
    Orbit(Grab),
    Pan(Pan),
}

pub struct Ivars {
    gpu: Gpu,
    radius: f32,
    orbit: Cell<Orbit>,
    drag: Cell<Drag>,
}

define_class!(
    // SAFETY: NSView's subclasses may override these event and layout methods, each with the
    // superclass's signature; the ivars need no particular drop order.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SolidCraftModelView"]
    #[ivars = Ivars]
    pub struct ModelView;

    unsafe impl NSObjectProtocol for ModelView {}

    // SAFETY: the method's signature matches NSGestureRecognizerDelegate's optional selector.
    unsafe impl NSGestureRecognizerDelegate for ModelView {
        #[unsafe(method(gestureRecognizer:shouldAttemptToRecognizeWithEvent:))]
        fn should_attempt(&self, _recognizer: &NSGestureRecognizer, event: &NSEvent) -> bool {
            event.modifierFlags().contains(NSEventModifierFlags::Option)
        }
    }

    impl ModelView {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let flags = event.modifierFlags();
            if event.clickCount() == 2 && flags.contains(NSEventModifierFlags::Option) {
                self.ivars().drag.set(Drag::None);
                return;
            }
            if flags.contains(NSEventModifierFlags::Shift) {
                self.start_pan(event);
            } else {
                let (x, y, width, height) = self.pointer(event);
                let drag = self.ivars().orbit.get().grab(x, y, width, height).map_or(Drag::None, Drag::Orbit);
                self.ivars().drag.set(drag);
            }
        }

        #[unsafe(method(resetView:))]
        fn reset_view(&self, _sender: Option<&AnyObject>) {
            self.ivars().drag.set(Drag::None);
            self.turn(Orbit::reset);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let (x, y, width, height) = self.pointer(event);
            match self.ivars().drag.get() {
                Drag::Orbit(grab) => self.turn(|orbit| orbit.drag_to(&grab, x, y, width, height)),
                Drag::Pan(pan) => self.turn(|orbit| orbit.pan_to(&pan, x, y, width, height)),
                Drag::None => {}
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            self.ivars().drag.set(Drag::None);
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            self.start_pan(event);
        }

        #[unsafe(method(rightMouseDragged:))]
        fn right_mouse_dragged(&self, event: &NSEvent) {
            self.continue_pan(event);
        }

        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, _event: &NSEvent) {
            self.ivars().drag.set(Drag::None);
        }

        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) {
            // Button 2 is the middle mouse button in AppKit (0 is left, 1 is right).
            if event.buttonNumber() != 2 {
                return;
            }
            if event.clickCount() == 2 {
                self.ivars().drag.set(Drag::None);
                self.turn(Orbit::reset);
                return;
            }
            self.start_pan(event);
        }

        #[unsafe(method(otherMouseDragged:))]
        fn other_mouse_dragged(&self, event: &NSEvent) {
            if event.buttonNumber() == 2 {
                self.continue_pan(event);
            }
        }

        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, _event: &NSEvent) {
            self.ivars().drag.set(Drag::None);
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            // A trackpad scrolls in points, a wheel in lines.
            let per = if event.hasPreciseScrollingDeltas() { 0.01 } else { 0.1 };
            self.zoom(event, (event.scrollingDeltaY() * per).exp() as f32);
        }

        #[unsafe(method(magnifyWithEvent:))]
        fn magnify(&self, event: &NSEvent) {
            self.zoom(event, (1.0 + event.magnification()) as f32);
        }

        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            // SAFETY: NSView's own `setFrameSize:` with the size AppKit handed us.
            let () = unsafe { msg_send![super(self), setFrameSize: size] };
            self.resize();
        }

        #[unsafe(method(viewDidChangeBackingProperties))]
        fn backing_changed(&self) {
            // SAFETY: NSView's own `viewDidChangeBackingProperties`, which takes nothing.
            let () = unsafe { msg_send![super(self), viewDidChangeBackingProperties] };
            self.resize();
        }
    }
);

impl ModelView {
    /// A view showing `model`, or `None` without a Metal device or when the shaders don't build.
    pub fn new(mtm: MainThreadMarker, model: Model) -> Option<Retained<Self>> {
        let gpu = Gpu::new(&model.vertices)?;
        let layer = gpu.layer.clone();
        let this = Self::alloc(mtm).set_ivars(Ivars { gpu, radius: model.radius, orbit: Cell::new(Orbit::default()), drag: Cell::new(Drag::None) });
        // SAFETY: NSView's designated initializer, with an empty frame the host resizes.
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // Layer-hosting: the view's layer is ours, set before the view wants one.
        view.setLayer(Some(&layer));
        view.setWantsLayer(true);
        // Claim Option + left double-click with a gesture recognizer so it resets the view without
        // opening the file in the app, while plain left double-click stays with Quick Look's host.
        // SAFETY: `view` implements `resetView:` with one optional sender argument and conforms to
        // `NSGestureRecognizerDelegate`; both `target` and `delegate` are weak references.
        let double_click = unsafe {
            NSClickGestureRecognizer::initWithTarget_action(NSClickGestureRecognizer::alloc(mtm), Some(view.as_ref()), Some(sel!(resetView:)))
        };
        double_click.setNumberOfClicksRequired(2);
        double_click.setDelegate(Some(ProtocolObject::from_ref(&*view)));
        view.addGestureRecognizer(&double_click);
        Some(view)
    }

    /// Start a pan drag at `event`'s pointer location.
    fn start_pan(&self, event: &NSEvent) {
        let (x, y, width, height) = self.pointer(event);
        let drag = self.ivars().orbit.get().pan_start(x, y, width, height).map_or(Drag::None, Drag::Pan);
        self.ivars().drag.set(drag);
    }

    /// Continue a pan drag in progress to `event`'s pointer location.
    fn continue_pan(&self, event: &NSEvent) {
        let Drag::Pan(pan) = self.ivars().drag.get() else { return };
        let (x, y, width, height) = self.pointer(event);
        self.turn(|orbit| orbit.pan_to(&pan, x, y, width, height));
    }

    /// Zoom by `factor`, re-anchoring any pan in progress so the model stays under the pointer.
    fn zoom(&self, event: &NSEvent, factor: f32) {
        self.turn(|orbit| orbit.zoom_by(factor));
        if matches!(self.ivars().drag.get(), Drag::Pan(_)) {
            self.start_pan(event);
        }
    }

    /// Where `event` happened in this view, in points from the bottom left, and the view's size.
    fn pointer(&self, event: &NSEvent) -> (f32, f32, f32, f32) {
        let at = self.convertPoint_fromView(event.locationInWindow(), None);
        let size = self.bounds().size;
        (at.x as f32, at.y as f32, size.width as f32, size.height as f32)
    }

    /// Change the orbit with `change`, then draw.
    fn turn(&self, change: impl FnOnce(&mut Orbit)) {
        let mut orbit = self.ivars().orbit.get();
        change(&mut orbit);
        self.ivars().orbit.set(orbit);
        self.draw();
    }

    /// Match the drawable to the view's size in pixels, then draw.
    fn resize(&self) {
        let size = self.bounds().size;
        let pixels = self.convertSizeToBacking(size);
        let layer = &self.ivars().gpu.layer;
        if size.width > 0.0 {
            layer.setContentsScale(pixels.width / size.width);
        }
        layer.setDrawableSize(CGSize::new(pixels.width.max(1.0), pixels.height.max(1.0)));
        self.draw();
    }

    fn draw(&self) {
        let ivars = self.ivars();
        ivars.gpu.draw(ivars.orbit.get().matrices(ivars.radius, aspect(self.bounds().size)));
    }
}

/// Width over height, or 1 for an empty view.
fn aspect(size: NSSize) -> f32 {
    if size.height > 0.0 && size.width > 0.0 { (size.width / size.height) as f32 } else { 1.0 }
}

impl Gpu {
    fn new(vertices: &[Vertex]) -> Option<Gpu> {
        let device = MTLCreateSystemDefaultDevice()?;
        let queue = device.newCommandQueue()?;
        let library = device.newLibraryWithSource_options_error(&NSString::from_str(SHADERS), None).ok()?;
        let descriptor = MTLRenderPipelineDescriptor::new();
        descriptor.setVertexFunction(library.newFunctionWithName(&NSString::from_str("corner")).as_deref());
        descriptor.setFragmentFunction(library.newFunctionWithName(&NSString::from_str("shade")).as_deref());
        // SAFETY: index 0 always exists in a pipeline descriptor's colour attachments.
        unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) }.setPixelFormat(COLOR_FORMAT);
        descriptor.setDepthAttachmentPixelFormat(DEPTH_FORMAT);
        let pipeline = device.newRenderPipelineStateWithDescriptor_error(&descriptor).ok()?;
        let depth = MTLDepthStencilDescriptor::new();
        depth.setDepthCompareFunction(MTLCompareFunction::Less);
        depth.setDepthWriteEnabled(true);
        let depth_test = device.newDepthStencilStateWithDescriptor(&depth)?;
        let bytes = std::mem::size_of_val(vertices);
        let start = NonNull::new(vertices.as_ptr().cast_mut())?.cast();
        // SAFETY: `start` points at `bytes` bytes of plain `f32`s, which Metal copies.
        let corners = unsafe { device.newBufferWithBytes_length_options(start, bytes, MTLResourceOptions::StorageModeShared) }?;
        let layer = CAMetalLayer::new();
        layer.setDevice(Some(&device));
        layer.setPixelFormat(COLOR_FORMAT);
        layer.setFramebufferOnly(true);
        layer.setOpaque(false);
        Some(Gpu { layer, queue, pipeline, depth_test, corners, count: vertices.len(), depth: RefCell::new(None) })
    }

    /// One frame seen through `matrices`.
    fn draw(&self, matrices: Matrices) {
        let size = self.layer.drawableSize();
        let (width, height) = (size.width as usize, size.height as usize);
        if width == 0 || height == 0 {
            return;
        }
        let Some(depth) = self.depth_texture(width, height) else { return };
        let Some(drawable) = self.layer.nextDrawable() else { return };
        let pass = MTLRenderPassDescriptor::renderPassDescriptor();
        // SAFETY: index 0 always exists in a pass descriptor's colour attachments.
        let color = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        color.setTexture(Some(&drawable.texture()));
        color.setLoadAction(MTLLoadAction::Clear);
        color.setClearColor(MTLClearColor { red: 0.0, green: 0.0, blue: 0.0, alpha: 0.0 });
        color.setStoreAction(MTLStoreAction::Store);
        let depth_attachment = pass.depthAttachment();
        depth_attachment.setTexture(Some(&depth));
        depth_attachment.setLoadAction(MTLLoadAction::Clear);
        depth_attachment.setClearDepth(1.0);
        depth_attachment.setStoreAction(MTLStoreAction::DontCare);
        let Some(commands) = self.queue.commandBuffer() else { return };
        let Some(encoder) = commands.renderCommandEncoderWithDescriptor(&pass) else { return };
        encoder.setRenderPipelineState(&self.pipeline);
        encoder.setDepthStencilState(Some(&self.depth_test));
        let Some(bytes) = NonNull::new(std::ptr::from_ref(&matrices).cast_mut()) else { return };
        // SAFETY: buffer 0 holds `count` corners, as the vertex function reads them; buffer 1 is
        // the matrices' bytes, copied by Metal before the call returns; the draw reads no further.
        unsafe {
            encoder.setVertexBuffer_offset_atIndex(Some(&self.corners), 0, 0);
            encoder.setVertexBytes_length_atIndex(bytes.cast(), std::mem::size_of::<Matrices>(), 1);
            encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, self.count);
        }
        encoder.endEncoding();
        commands.presentDrawable(ProtocolObject::from_ref(&*drawable));
        commands.commit();
    }

    /// The depth buffer for a `width` x `height` drawable (made again when the size changes).
    fn depth_texture(&self, width: usize, height: usize) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        let mut depth = self.depth.borrow_mut();
        if let Some(texture) = depth.as_ref()
            && texture.width() == width
            && texture.height() == height
        {
            return Some(texture.clone());
        }
        // SAFETY: a 2D depth format with sizes from the drawable, which Metal accepts.
        let descriptor =
            unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(DEPTH_FORMAT, width, height, false) };
        descriptor.setStorageMode(MTLStorageMode::Private);
        descriptor.setUsage(MTLTextureUsage::RenderTarget);
        let texture = self.queue.device().newTextureWithDescriptor(&descriptor)?;
        *depth = Some(texture.clone());
        Some(texture)
    }
}
