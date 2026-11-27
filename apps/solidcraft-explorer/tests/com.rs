//! The handler through COM, as Windows calls it, with nothing registered. The test loads the
//! built DLL from beside itself (`cargo test` puts the cdylib in `target/<profile>/deps/`), asks it
//! for its class factory, makes a handler, gives it a design in a memory stream and asks for a
//! thumbnail. Windows only: CI's Windows job runs it, and locally the cross-built test `.exe` runs
//! through WSL interop.
#![cfg(windows)]
#![allow(unsafe_code)]

mod support;

use std::ffi::{CStr, c_void};
use std::sync::{Mutex, MutexGuard, OnceLock};

use support::{DOCUMENT, V1, V2};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_INVALIDARG, E_POINTER, E_UNEXPECTED, HMODULE, S_FALSE, S_OK,
};
use windows::Win32::Graphics::Gdi::{DIBSECTION, DeleteObject, GetObjectW, HBITMAP, HGDIOBJ};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, IClassFactory, STGM_READ};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Shell::PropertiesSystem::IInitializeWithStream;
use windows::Win32::UI::Shell::{IThumbnailProvider, SHCreateMemStream, WTS_ALPHATYPE, WTSAT_ARGB};
use windows::core::{GUID, HRESULT, HSTRING, IUnknown, Interface, PCSTR};

type GetClassObject = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
type CanUnloadNow = unsafe extern "system" fn() -> HRESULT;

/// `HRESULT_FROM_WIN32(ERROR_ALREADY_INITIALIZED)`.
const ALREADY_INITIALIZED: HRESULT = HRESULT(0x8007_04DF_u32 as i32);

/// One test at a time, each in a COM apartment: they share the DLL's count of live objects.
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    guard
}

/// The DLL, loaded once for the test binary and never unloaded.
fn dll() -> HMODULE {
    static DLL: OnceLock<usize> = OnceLock::new();
    let raw = *DLL.get_or_init(|| {
        let exe = std::env::current_exe().unwrap();
        let deps = exe.parent().unwrap();
        let path = [deps.join("solidcraft_explorer.dll"), deps.parent().unwrap().join("solidcraft_explorer.dll")]
            .into_iter()
            .find(|p| p.exists())
            .expect("solidcraft_explorer.dll beside the test executable");
        let module = unsafe { LoadLibraryW(&HSTRING::from(path.as_os_str())) }.expect("the DLL loads");
        module.0 as usize
    });
    HMODULE(raw as *mut c_void)
}

fn export(name: &CStr) -> unsafe extern "system" fn() -> isize {
    unsafe { GetProcAddress(dll(), PCSTR(name.as_ptr().cast())) }.expect("the function is exported")
}

fn get_class_object(clsid: &GUID, out: *mut *mut c_void) -> HRESULT {
    let get: GetClassObject = unsafe { std::mem::transmute(export(c"DllGetClassObject")) };
    unsafe { get(clsid, &IClassFactory::IID, out) }
}

fn can_unload() -> HRESULT {
    let can: CanUnloadNow = unsafe { std::mem::transmute(export(c"DllCanUnloadNow")) };
    unsafe { can() }
}

fn factory() -> IClassFactory {
    let mut out = std::ptr::null_mut();
    assert_eq!(get_class_object(&GUID::from_u128(solidcraft_explorer::CLSID_U128), &mut out), S_OK);
    unsafe { IClassFactory::from_raw(out) }
}

/// A handler initialised with `bytes` as its stream.
fn handler(factory: &IClassFactory, bytes: &[u8]) -> IThumbnailProvider {
    unsafe {
        let init: IInitializeWithStream = factory.CreateInstance(None).unwrap();
        let stream = SHCreateMemStream(Some(bytes)).unwrap();
        init.Initialize(&stream, STGM_READ.0).unwrap();
        init.cast().unwrap()
    }
}

/// What `GetThumbnail(cx)` returns for `bytes`; the bitmap handle starts as a non-null dummy, to
/// show a failure clears it.
fn thumbnail(factory: &IClassFactory, bytes: &[u8], cx: u32) -> (windows::core::Result<()>, HBITMAP, WTS_ALPHATYPE) {
    let provider = handler(factory, bytes);
    let mut hbmp = HBITMAP(std::ptr::dangling_mut::<c_void>());
    let mut alpha = WTS_ALPHATYPE(7);
    let result = unsafe { provider.GetThumbnail(cx, &mut hbmp, &mut alpha) };
    (result, hbmp, alpha)
}

/// The bitmap's width, height, bits a pixel and bytes (first row in memory first); then deletes it.
fn pixels(hbmp: HBITMAP) -> (i32, i32, u16, Vec<u8>) {
    let mut section = DIBSECTION::default();
    let size = std::mem::size_of::<DIBSECTION>() as i32;
    let n = unsafe { GetObjectW(HGDIOBJ(hbmp.0), size, Some((&mut section as *mut DIBSECTION).cast())) };
    assert_eq!(n, size, "a DIB section");
    let (w, h) = (section.dsBm.bmWidth, section.dsBm.bmHeight.abs());
    let bytes = unsafe { std::slice::from_raw_parts(section.dsBm.bmBits as *const u8, (w * h * 4) as usize) }.to_vec();
    let _ = unsafe { DeleteObject(HGDIOBJ(hbmp.0)) };
    (w, h, section.dsBm.bmBitsPixel, bytes)
}

#[test]
fn a_saved_picture_comes_back_as_a_top_down_bitmap() {
    let _serial = serial();
    let factory = factory();
    for (tag, w, h, cx, want) in
        [(V2, 512, 384, 256, (256, 192)), (V2, 512, 384, 1024, (512, 384)), (V1, 240, 150, 96, (96, 60)), (V1, 240, 150, 256, (240, 150))]
    {
        let (result, hbmp, alpha) = thumbnail(&factory, &support::design(tag, w, h), cx);
        assert_eq!(result, Ok(()), "{tag} at {cx}");
        assert_eq!(alpha, WTSAT_ARGB);
        let (width, height, bits, bytes) = pixels(hbmp);
        assert_eq!(((width, height), bits), (want, 32), "{tag} at {cx}");
        let row = width as usize * 4;
        // Memory row 0 is the picture's top row: green at the left, transparent at the right.
        assert_eq!(bytes[0..4], [0, 255, 0, 255]);
        assert_eq!(bytes[row - 1], 0);
        // The last memory row is the bottom: red at the left.
        assert_eq!(bytes[bytes.len() - row..bytes.len() - row + 4], [0, 0, 255, 255]);
    }
}

#[test]
fn a_design_without_a_usable_picture_fails_and_returns_no_bitmap() {
    let _serial = serial();
    let factory = factory();
    let stale = support::keyed_to(V2, 512, 384, b"{}");
    let many = support::many_entries(&support::design(V2, 512, 384));
    for (what, bytes) in [
        ("stale key", stale),
        ("not a zip", b"not a zip".to_vec()),
        ("plain JSON", DOCUMENT.to_vec()),
        ("bomb", support::bomb()),
        ("many entries", many),
    ] {
        let (result, hbmp, alpha) = thumbnail(&factory, &bytes, 256);
        assert_eq!(result.map_err(|e| e.code()), Err(E_FAIL), "{what}");
        assert!(hbmp.0.is_null() && alpha == WTS_ALPHATYPE(0), "{what}: the out-parameters are cleared");
    }
}

#[test]
fn the_handler_refuses_wrong_calls() {
    let _serial = serial();
    let factory = factory();
    let design = support::design(V2, 512, 384);
    let (result, _, _) = thumbnail(&factory, &design, 0);
    assert_eq!(result.map_err(|e| e.code()), Err(E_INVALIDARG));
    let provider = handler(&factory, &design);
    let mut alpha = WTS_ALPHATYPE(0);
    let result = unsafe { provider.GetThumbnail(256, std::ptr::null_mut(), &mut alpha) };
    assert_eq!(result.map_err(|e| e.code()), Err(E_POINTER));
    let uninitialised: IThumbnailProvider = unsafe { factory.CreateInstance(None) }.unwrap();
    let mut hbmp = HBITMAP::default();
    let result = unsafe { uninitialised.GetThumbnail(256, &mut hbmp, &mut alpha) };
    assert_eq!(result.map_err(|e| e.code()), Err(E_UNEXPECTED));
    let init: IInitializeWithStream = provider.cast().unwrap();
    let stream = unsafe { SHCreateMemStream(Some(&design)) }.unwrap();
    let again = unsafe { init.Initialize(&stream, STGM_READ.0) };
    assert_eq!(again.map_err(|e| e.code()), Err(ALREADY_INITIALIZED));
}

#[test]
fn the_factory_refuses_other_classes_and_aggregation() {
    let _serial = serial();
    let mut out = std::ptr::dangling_mut::<c_void>();
    assert_eq!(get_class_object(&GUID::from_u128(1), &mut out), CLASS_E_CLASSNOTAVAILABLE);
    assert!(out.is_null());
    assert_eq!(get_class_object(&GUID::from_u128(solidcraft_explorer::CLSID_U128), std::ptr::null_mut()), E_POINTER);
    let factory = factory();
    let outer: IUnknown = unsafe { SHCreateMemStream(Some(b"x")) }.unwrap().cast().unwrap();
    let made: windows::core::Result<IThumbnailProvider> = unsafe { factory.CreateInstance(&outer) };
    assert_eq!(made.map(|_| ()).map_err(|e| e.code()), Err(CLASS_E_NOAGGREGATION));
}

#[test]
fn the_dll_can_unload_only_when_nothing_is_alive() {
    let _serial = serial();
    assert_eq!(can_unload(), S_OK);
    let factory = factory();
    assert_eq!(can_unload(), S_FALSE, "a factory is alive");
    let provider = handler(&factory, &support::design(V2, 512, 384));
    drop(factory);
    assert_eq!(can_unload(), S_FALSE, "a handler is alive");
    drop(provider);
    assert_eq!(can_unload(), S_OK);
    let factory = self::factory();
    unsafe { factory.LockServer(true) }.unwrap();
    drop(factory);
    assert_eq!(can_unload(), S_FALSE, "the server is locked");
    let factory = self::factory();
    unsafe { factory.LockServer(false) }.unwrap();
    drop(factory);
    assert_eq!(can_unload(), S_OK);
}

#[test]
fn every_out_parameter_is_cleared_when_another_argument_is_null() {
    let _serial = serial();
    let get: GetClassObject = unsafe { std::mem::transmute(export(c"DllGetClassObject")) };
    let clsid = GUID::from_u128(solidcraft_explorer::CLSID_U128);
    let mut out = std::ptr::dangling_mut::<c_void>();
    assert_eq!(unsafe { get(&clsid, std::ptr::null(), &mut out) }, E_POINTER, "DllGetClassObject without an interface id");
    assert!(out.is_null(), "DllGetClassObject clears its out-parameter");
    let mut out = std::ptr::dangling_mut::<c_void>();
    assert_eq!(unsafe { get(std::ptr::null(), &IClassFactory::IID, &mut out) }, E_POINTER, "DllGetClassObject without a class id");
    assert!(out.is_null(), "DllGetClassObject clears its out-parameter");
    let factory = factory();
    let mut out = std::ptr::dangling_mut::<c_void>();
    let create = Interface::vtable(&factory).CreateInstance;
    assert_eq!(unsafe { create(Interface::as_raw(&factory), std::ptr::null_mut(), std::ptr::null(), &mut out) }, E_POINTER);
    assert!(out.is_null(), "CreateInstance clears its out-parameter");
    let provider = handler(&factory, &support::design(V2, 512, 384));
    let mut hbmp = HBITMAP(std::ptr::dangling_mut::<c_void>());
    let result = unsafe { provider.GetThumbnail(256, &mut hbmp, std::ptr::null_mut()) };
    assert_eq!(result.map_err(|e| e.code()), Err(E_POINTER));
    assert!(hbmp.0.is_null(), "GetThumbnail clears the bitmap when the alpha type has nowhere to go");
    let mut alpha = WTS_ALPHATYPE(7);
    let result = unsafe { provider.GetThumbnail(256, std::ptr::null_mut(), &mut alpha) };
    assert_eq!(result.map_err(|e| e.code()), Err(E_POINTER));
    assert_eq!(alpha, WTS_ALPHATYPE(0), "GetThumbnail clears the alpha type when the bitmap has nowhere to go");
}
