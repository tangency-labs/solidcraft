//! Windows: finding 3D mice without asking any other device anything (#2614).
//!
//! hidapi's enumeration opens every HID interface and reads its serial number, manufacturer and
//! product strings: on USB, three control requests to every keyboard, mouse and touchscreen. The
//! reader repeated it every two seconds while no 3D mouse was plugged in, and on some machines the
//! keyboard then kept dropping off the bus and the cursor jumped. Plug and Play already names each
//! interface's top-level usage in its hardware IDs (`HID_DEVICE_UP:0001_U:0008`: Generic Desktop,
//! Multi-Axis Controller), so the reader asks it, which sends nothing to any device, and opens only
//! the interfaces that are 3D mice.
//!
//! The configuration manager has no safe API, hence the `unsafe_code` allowance on this module
//! (the app crate denies it; see `native_crash.rs`).
#![allow(unsafe_code)]

use std::ffi::CString;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_Get_DevNode_PropertyW, CM_Get_Device_Interface_List_SizeW, CM_Get_Device_Interface_ListW,
    CM_Get_Device_Interface_PropertyW, CM_LOCATE_DEVNODE_NORMAL, CM_Locate_DevNodeW, CONFIGRET, CR_BUFFER_SMALL, CR_SUCCESS,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::GUID_DEVINTERFACE_HID;
use windows_sys::Win32::Devices::Properties::{
    DEVPKEY_Device_HardwareIds, DEVPKEY_Device_InstanceId, DEVPROP_TYPE_STRING, DEVPROP_TYPE_STRING_LIST, DEVPROPTYPE,
};
use windows_sys::Win32::Foundation::DEVPROPKEY;

/// The hardware ID Windows gives a top-level collection with the Generic Desktop page's
/// Multi-Axis Controller usage.
const MULTI_AXIS_CONTROLLER: &str = "HID_DEVICE_UP:0001_U:0008";
/// Largest property read, in bytes: hardware IDs are a few hundred.
const MAX_PROPERTY_BYTES: u32 = 64 * 1024;
/// Tries at reading the interface list, which can grow between asking its size and reading it.
const LIST_TRIES: usize = 4;

/// The paths of the present HID interfaces whose top-level collection is a multi-axis controller.
pub fn multi_axis_controllers() -> Vec<CString> {
    interfaces()
        .into_iter()
        .filter(|path| {
            let Some(instance) = interface_strings(path, &DEVPKEY_Device_InstanceId).and_then(|ids| ids.into_iter().next()) else {
                return false;
            };
            devnode_strings(&instance, &DEVPKEY_Device_HardwareIds).is_some_and(|ids| is_multi_axis(&ids))
        })
        .filter_map(|path| CString::new(path).ok())
        .collect()
}

/// Whether a device with these hardware IDs is a multi-axis controller.
fn is_multi_axis(hardware_ids: &[String]) -> bool {
    hardware_ids.iter().any(|id| id.eq_ignore_ascii_case(MULTI_AXIS_CONTROLLER))
}

/// The strings of a nul-separated list (`REG_MULTI_SZ`, or one string), without empty ones.
fn split_multi_sz(buffer: &[u16]) -> Vec<String> {
    buffer.split(|c| *c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
}

/// `s` as a nul-terminated wide string.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The paths of the present HID interfaces.
fn interfaces() -> Vec<String> {
    for _ in 0..LIST_TRIES {
        let mut len = 0u32;
        // SAFETY: `len` is a valid place for the size; the class GUID is a static; no device filter.
        let sized = unsafe { CM_Get_Device_Interface_List_SizeW(&mut len, &GUID_DEVINTERFACE_HID, null(), CM_GET_DEVICE_INTERFACE_LIST_PRESENT) };
        if sized != CR_SUCCESS || len == 0 {
            return Vec::new();
        }
        let mut buffer = vec![0u16; len as usize];
        // SAFETY: `buffer` holds `len` wide characters, the length the call is given.
        let listed =
            unsafe { CM_Get_Device_Interface_ListW(&GUID_DEVINTERFACE_HID, null(), buffer.as_mut_ptr(), len, CM_GET_DEVICE_INTERFACE_LIST_PRESENT) };
        match listed {
            CR_SUCCESS => return split_multi_sz(&buffer),
            CR_BUFFER_SMALL => continue,
            _ => return Vec::new(),
        }
    }
    Vec::new()
}

/// A string or string-list property, read by `get` (property type, buffer, its size in bytes) in
/// two calls: the size, then the value.
fn property_strings(get: impl Fn(*mut DEVPROPTYPE, *mut u8, *mut u32) -> CONFIGRET) -> Option<Vec<String>> {
    let mut kind: DEVPROPTYPE = 0;
    let mut size = 0u32;
    if get(&mut kind, null_mut(), &mut size) != CR_BUFFER_SMALL || size == 0 || size > MAX_PROPERTY_BYTES {
        return None;
    }
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    let mut size = (buffer.len() * 2) as u32;
    if get(&mut kind, buffer.as_mut_ptr().cast(), &mut size) != CR_SUCCESS || (kind != DEVPROP_TYPE_STRING && kind != DEVPROP_TYPE_STRING_LIST) {
        return None;
    }
    Some(split_multi_sz(&buffer))
}

/// A string property of the device interface at `path`.
fn interface_strings(path: &str, key: &DEVPROPKEY) -> Option<Vec<String>> {
    let path = wide(path);
    property_strings(|kind, buffer, size| {
        // SAFETY: `path` is nul-terminated and outlives the call; `buffer` is null (asking the size)
        // or holds `*size` bytes, as `property_strings` sets up.
        unsafe { CM_Get_Device_Interface_PropertyW(path.as_ptr(), key, kind, buffer, size, 0) }
    })
}

/// A string property of the device with instance ID `instance`.
fn devnode_strings(instance: &str, key: &DEVPROPKEY) -> Option<Vec<String>> {
    let instance = wide(instance);
    let mut devnode = 0u32;
    // SAFETY: `instance` is nul-terminated and outlives the call; `devnode` is a valid place for the result.
    if unsafe { CM_Locate_DevNodeW(&mut devnode, instance.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) } != CR_SUCCESS {
        return None;
    }
    property_strings(|kind, buffer, size| {
        // SAFETY: `buffer` is null (asking the size) or holds `*size` bytes, as `property_strings` sets up.
        unsafe { CM_Get_DevNode_PropertyW(devnode, key, kind, buffer, size, 0) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hardware IDs Windows gives a wired 3Dconnexion device's motion interface, and a
    /// keyboard's: only the first is a multi-axis controller.
    #[test]
    fn only_the_multi_axis_usage_is_a_3d_mouse() {
        let ids = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let space_mouse = ids(&["HID\\VID_256F&PID_C635&REV_0437&MI_00", "HID\\VID_256F&UP:0001_U:0008", "HID_DEVICE_UP:0001_U:0008", "HID_DEVICE"]);
        let keyboard = ids(&[
            "HID\\VID_046D&PID_C31C&REV_6400&MI_00",
            "HID\\VID_046D&UP:0001_U:0006",
            "HID_DEVICE_SYSTEM_KEYBOARD",
            "HID_DEVICE_UP:0001_U:0006",
            "HID_DEVICE",
        ]);
        let vendor = ids(&["HID\\VID_256F&UP:FF00_U:0001", "HID_DEVICE_UP:FF00_U:0001", "HID_DEVICE_UPR:FF00-FFFF", "HID_DEVICE"]);
        assert!(is_multi_axis(&space_mouse));
        assert!(is_multi_axis(&ids(&["hid_device_up:0001_u:0008"])), "IDs compare without regard to case");
        assert!(!is_multi_axis(&keyboard) && !is_multi_axis(&vendor) && !is_multi_axis(&[]));
    }

    #[test]
    fn nul_separated_lists_split_into_their_strings() {
        let buffer: Vec<u16> = "a\\b\0HID_DEVICE\0\0".encode_utf16().collect();
        assert_eq!(split_multi_sz(&buffer), ["a\\b", "HID_DEVICE"]);
        assert_eq!(split_multi_sz(&wide("one")), ["one"]);
        assert!(split_multi_sz(&[0, 0]).is_empty() && split_multi_sz(&[]).is_empty());
    }

    /// The scan asks Plug and Play only: on any machine it returns, and every path it returns is a
    /// present HID interface whose hardware IDs name the multi-axis usage.
    #[test]
    fn the_scan_returns_only_multi_axis_interfaces() {
        let found = multi_axis_controllers();
        let all = interfaces();
        for path in &found {
            let path = path.to_str().unwrap_or_default();
            assert!(all.iter().any(|p| p == path), "{path}");
            let instance = interface_strings(path, &DEVPKEY_Device_InstanceId).and_then(|ids| ids.into_iter().next()).unwrap_or_default();
            assert!(devnode_strings(&instance, &DEVPKEY_Device_HardwareIds).is_some_and(|ids| is_multi_axis(&ids)), "{path}");
        }
        eprintln!("{} HID interfaces present, {} multi-axis controllers", all.len(), found.len());
    }
}
