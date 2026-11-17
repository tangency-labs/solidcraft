//! The calls that have no safe API: opening the OpenXR loader, having the runtime create the
//! Vulkan instance and device, handing those to wgpu, opening the session on them, and wrapping
//! the runtime's pictures as wgpu textures. Each is one function with its `SAFETY` note; nothing
//! else in the crate is `unsafe`.
#![allow(unsafe_code)]

use std::ffi::{CStr, c_char};

use ash::vk::{self, Handle as _};
use openxr as xr;
use wgpu::hal;

use super::{Error, Gpu};

/// The Vulkan objects a session is opened on, as plain handles.
#[derive(Clone, Copy)]
pub(super) struct RawGraphics {
    instance: vk::Instance,
    physical_device: vk::PhysicalDevice,
    device: vk::Device,
    queue_family: u32,
}

/// The file name of the OpenXR loader on this system.
#[cfg(target_os = "windows")]
const LOADER: &str = "openxr_loader.dll";
#[cfg(target_os = "macos")]
const LOADER: &str = "libopenxr_loader.dylib";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const LOADER: &str = "libopenxr_loader.so.1";

/// Open the OpenXR loader: the one beside the program first (on Windows the loader is a library
/// an app brings along, not part of the system), then the one on the system's search path.
pub(super) fn load_entry() -> Result<xr::Entry, Error> {
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join(LOADER))).filter(|path| path.is_file());
    // SAFETY: both calls load a library named for the OpenXR loader and look up its documented
    // entry points; a library by that name beside the program or on the search path that is not
    // the Khronos loader is a broken install, as it is for every OpenXR app.
    let loaded = unsafe {
        match &beside {
            Some(path) => xr::Entry::load_from(path, &()),
            None => xr::Entry::load(&()),
        }
    };
    loaded.map_err(|e| match e {
        xr::EntryError::Load(e) => Error::NoLoader(format!("{LOADER}: {e}")),
        xr::EntryError::Xr(code) => Error::NoRuntime(code.to_string()),
    })
}

/// OpenXR's name for Vulkan's `vkGetInstanceProcAddr`, from ash's.
fn proc_addr(entry: &ash::Entry) -> xr::sys::platform::VkGetInstanceProcAddr {
    // SAFETY: both are the C function `vkGetInstanceProcAddr`; the two crates spell its type with
    // their own names for the same handle and pointer types.
    unsafe {
        std::mem::transmute::<vk::PFN_vkGetInstanceProcAddr, xr::sys::platform::VkGetInstanceProcAddr>(entry.static_fn().get_instance_proc_addr)
    }
}

/// A Vulkan failure as text.
fn vulkan_error(what: &str, code: i32) -> Error {
    Error::Vulkan(format!("{what}: {:?}", vk::Result::from_raw(code)))
}

/// Have the runtime create the Vulkan instance and device for `system`, and hand them to wgpu:
/// the instance with the extensions wgpu wants (its window surfaces among them), the device with
/// the features and limits `describe` asks of the adapter the runtime names. wgpu owns both from
/// here and destroys them when its last handle goes.
pub(super) fn open_graphics(
    instance: &xr::Instance,
    system: xr::SystemId,
    describe: &dyn Fn(&wgpu::Adapter) -> wgpu::DeviceDescriptor<'static>,
) -> Result<(Gpu, RawGraphics), Error> {
    let requirements = instance.graphics_requirements::<xr::Vulkan>(system).map_err(|e| Error::xr("graphics requirements", e))?;
    // The Vulkan version wgpu-hal builds for is 1.3.
    let target = xr::Version::new(1, 3, 0);
    if requirements.min_api_version_supported > target || requirements.max_api_version_supported.major() < target.major() {
        return Err(Error::Vulkan(format!(
            "the VR runtime wants Vulkan {} to {}, SolidCraft draws with 1.3",
            requirements.min_api_version_supported, requirements.max_api_version_supported
        )));
    }
    // SAFETY: loads the system's Vulkan loader, as wgpu itself does to start its Vulkan backend.
    let entry = unsafe { ash::Entry::load() }.map_err(|e| Error::Vulkan(format!("no Vulkan on this system: {e}")))?;
    // SAFETY: `entry` holds the loader's entry points.
    let instance_version = match unsafe { entry.try_enumerate_instance_version() } {
        Ok(Some(version)) => version,
        Ok(None) => vk::API_VERSION_1_0,
        Err(e) => return Err(Error::Vulkan(format!("the Vulkan version could not be read: {e:?}"))),
    };
    if instance_version < vk::API_VERSION_1_1 {
        return Err(Error::Vulkan("this system's Vulkan is older than 1.1".into()));
    }
    let flags = wgpu::InstanceFlags::empty();
    let extensions = hal::vulkan::Instance::desired_extensions(&entry, instance_version, flags).map_err(|e| Error::Vulkan(e.to_string()))?;
    // SAFETY: `entry` holds the loader's entry points.
    let layers = unsafe { entry.enumerate_instance_layer_properties() }.unwrap_or_default();
    let has_nv_optimus = layers.iter().any(|layer| layer.layer_name_as_c_str() == Ok(c"VK_LAYER_NV_optimus"));
    let app = vk::ApplicationInfo::default()
        .application_name(c"SolidCraft")
        .application_version(1)
        .engine_name(c"wgpu-hal")
        .engine_version(2)
        .api_version(vk::API_VERSION_1_3);
    let extension_names: Vec<*const c_char> = extensions.iter().map(|name: &&'static CStr| name.as_ptr()).collect();
    let create = vk::InstanceCreateInfo::default().application_info(&app).enabled_extension_names(&extension_names);
    // SAFETY: `create` is a valid `VkInstanceCreateInfo` that lives across the call, with the
    // loader's own `vkGetInstanceProcAddr`; the graphics requirements were asked for above, as
    // `XR_KHR_vulkan_enable2` requires. The runtime adds the extensions it needs itself.
    let raw_instance = unsafe { instance.create_vulkan_instance(system, proc_addr(&entry), std::ptr::from_ref(&create).cast()) }
        .map_err(|e| Error::xr("creating the Vulkan instance", e))?
        .map_err(|code| vulkan_error("creating the Vulkan instance", code))?;
    let vk_instance = vk::Instance::from_raw(raw_instance as u64);
    // SAFETY: `vk_instance` was just created through `entry`'s loader.
    let ash_instance = unsafe { ash::Instance::load(entry.static_fn(), vk_instance) };
    // SAFETY: the instance comes from `create_vulkan_instance` on this runtime and system.
    let physical_device = match unsafe { instance.vulkan_graphics_device(system, raw_instance) } {
        Ok(device) => vk::PhysicalDevice::from_raw(device as u64),
        Err(e) => {
            // SAFETY: nothing else holds the instance yet.
            unsafe { ash_instance.destroy_instance(None) };
            return Err(Error::xr("finding the headset's graphics card", e));
        }
    };
    // SAFETY: `ash_instance` was created from `entry` with `extensions` (a superset: the runtime
    // only adds), the version and the flags given here; with no drop callback wgpu-hal owns it
    // from now on, and destroys it when the wgpu instance goes.
    let hal_instance = unsafe {
        hal::vulkan::Instance::from_raw(
            entry.clone(),
            ash_instance.clone(),
            instance_version,
            0,
            None,
            extensions,
            flags,
            wgpu::MemoryBudgetThresholds::default(),
            has_nv_optimus,
            None,
        )
    }
    .map_err(|e| Error::Vulkan(e.to_string()))?;
    let exposed = hal_instance
        .expose_adapter(physical_device)
        .ok_or_else(|| Error::Vulkan("the headset's graphics card cannot draw SolidCraft's view".into()))?;
    // SAFETY: a wgpu-hal Vulkan instance made just above; wgpu takes it over whole.
    let wgpu_instance = unsafe { wgpu::Instance::from_hal::<hal::api::Vulkan>(hal_instance) };
    // SAFETY: `exposed` is an adapter of that very instance.
    let adapter = unsafe { wgpu_instance.create_adapter_from_hal(exposed) };
    let descriptor = describe(&adapter);
    let missing = descriptor.required_features - adapter.features();
    if !missing.is_empty() {
        return Err(Error::Vulkan(format!("the headset's graphics card lacks {missing:?}")));
    }
    // SAFETY: the physical device belongs to `ash_instance`.
    let families = unsafe { ash_instance.get_physical_device_queue_family_properties(physical_device) };
    let queue_family = families
        .iter()
        .position(|family| family.queue_flags.contains(vk::QueueFlags::GRAPHICS))
        .and_then(|index| u32::try_from(index).ok())
        .ok_or_else(|| Error::Vulkan("the headset's graphics card has no graphics queue".into()))?;
    let open = {
        // SAFETY: the adapter is a Vulkan one, made above.
        let hal_adapter = unsafe { adapter.as_hal::<hal::api::Vulkan>() }.ok_or_else(|| Error::Vulkan("the adapter is not a Vulkan one".into()))?;
        let device_extensions = hal_adapter.required_device_extensions(descriptor.required_features);
        let mut features = hal_adapter.physical_device_features(&device_extensions, descriptor.required_features);
        let priorities = [1.0];
        let queues = [vk::DeviceQueueCreateInfo::default().queue_family_index(queue_family).queue_priorities(&priorities)];
        let names: Vec<*const c_char> = device_extensions.iter().map(|name: &&'static CStr| name.as_ptr()).collect();
        let create = features.add_to_device_create(vk::DeviceCreateInfo::default().queue_create_infos(&queues).enabled_extension_names(&names));
        // SAFETY: `create` is a valid `VkDeviceCreateInfo` that lives across the call, for the
        // physical device the runtime named, on the instance it created.
        let raw_device =
            unsafe { instance.create_vulkan_device(system, proc_addr(&entry), physical_device.as_raw() as _, std::ptr::from_ref(&create).cast()) }
                .map_err(|e| Error::xr("creating the Vulkan device", e))?
                .map_err(|code| vulkan_error("creating the Vulkan device", code))?;
        let vk_device = vk::Device::from_raw(raw_device as u64);
        // SAFETY: `vk_device` was just created on `ash_instance`.
        let ash_device = unsafe { ash::Device::load(ash_instance.fp_v1_0(), vk_device) };
        // Kept to destroy the device if wgpu-hal does not take it.
        let unowned = ash_device.clone();
        // SAFETY: the device was created from this adapter with `queue_family`, the extensions
        // and the features worked out above (the runtime only adds extensions); with no drop
        // callback wgpu-hal owns it from now on.
        let open = unsafe {
            hal_adapter.device_from_raw(
                ash_device,
                None,
                &device_extensions,
                descriptor.required_features,
                &descriptor.required_limits,
                &descriptor.memory_hints,
                queue_family,
                0,
            )
        }
        .map_err(|e| {
            // SAFETY: wgpu-hal refused the device, so nothing else holds it, and nothing was made
            // on it yet.
            unsafe { unowned.destroy_device(None) };
            Error::Vulkan(e.to_string())
        })?;
        (open, vk_device)
    };
    let (open, vk_device) = open;
    // SAFETY: `open` was opened from this adapter with the features, limits and memory hints
    // that `descriptor` names.
    let (device, queue) = unsafe { adapter.create_device_from_hal(open, &descriptor) }.map_err(|e| Error::Vulkan(e.to_string()))?;
    let raw = RawGraphics { instance: vk_instance, physical_device, device: vk_device, queue_family };
    Ok((Gpu { instance: wgpu_instance, adapter, device, queue }, raw))
}

/// Open the session on the device [`open_graphics`] made.
pub(super) fn create_session(
    instance: &xr::Instance,
    system: xr::SystemId,
    raw: RawGraphics,
) -> Result<(xr::Session<xr::Vulkan>, xr::FrameWaiter, xr::FrameStream<xr::Vulkan>), Error> {
    let info = xr::vulkan::SessionCreateInfo {
        instance: raw.instance.as_raw() as _,
        physical_device: raw.physical_device.as_raw() as _,
        device: raw.device.as_raw() as _,
        queue_family_index: raw.queue_family,
        queue_index: 0,
    };
    // SAFETY: the handles are the instance, physical device and device the runtime itself
    // created for this system in `open_graphics`, alive for as long as the `Gpu` the session is
    // kept beside (`Session` drops its runtime handles before its device). Frames are begun,
    // drawn and ended on one thread, the window's.
    unsafe { instance.create_session::<xr::Vulkan>(system, &info) }.map_err(|e| Error::xr("opening the session", e))
}

/// One of the runtime's pictures as a wgpu texture to draw into. The runtime keeps the picture:
/// dropping the texture frees nothing of it.
pub(super) fn wrap_image(device: &wgpu::Device, image: u64, format: wgpu::TextureFormat, size: [u32; 2]) -> Option<wgpu::Texture> {
    let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
    let hal_descriptor = hal::TextureDescriptor {
        label: Some("sc_xr_picture"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUses::COLOR_TARGET,
        memory_flags: hal::MemoryFlags::empty(),
        view_formats: Vec::new(),
    };
    let descriptor = wgpu::TextureDescriptor {
        label: Some("sc_xr_picture"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    };
    // SAFETY: the device is the Vulkan one the session was opened on.
    let hal_device = unsafe { device.as_hal::<hal::api::Vulkan>() }?;
    // SAFETY: `image` is a picture of a swapchain the runtime created with this format and size,
    // one sample, one level and colour-attachment use. The drop callback and the external memory
    // tell wgpu-hal that the image and its memory are not its own to destroy: the runtime frees
    // them with the swapchain, which `Session` keeps until every texture made here is gone.
    let hal_texture = unsafe {
        hal_device.texture_from_raw(vk::Image::from_raw(image), &hal_descriptor, Some(Box::new(|| {})), hal::vulkan::TextureMemory::External)
    };
    drop(hal_device);
    // SAFETY: the texture was made on this device, with a descriptor that says what it is.
    Some(unsafe { device.create_texture_from_hal::<hal::api::Vulkan>(hal_texture, &descriptor, wgpu::TextureUses::UNINITIALIZED) })
}
