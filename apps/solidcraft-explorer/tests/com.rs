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
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_NOTIMPL, E_POINTER, E_UNEXPECTED, HMODULE, S_FALSE, S_OK,
};
use windows::Win32::Graphics::Gdi::{DIBSECTION, DeleteObject, GetObjectW, HBITMAP, HGDIOBJ};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, CoInitializeEx, IClassFactory, IStream, LOCKTYPE, STATFLAG, STATSTG, STGC, STGM_READ, STREAM_SEEK, STREAM_SEEK_CUR,
    STREAM_SEEK_END, STREAM_SEEK_SET,
};
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

/// How a [`TestStream`] misbehaves: every `Read` or `Seek` from the given call on fails, `Read`
/// claims 4 KiB more than it was asked for, or the stream reports a length of its own.
#[derive(Clone, Copy, Default)]
struct Behaviour {
    fail_read_from: Option<u32>,
    fail_seek_from: Option<u32>,
    overclaim: bool,
    length: Option<u64>,
}

/// How often the handler called the stream.
#[derive(Default)]
struct Calls {
    reads: std::sync::atomic::AtomicU32,
    seeks: std::sync::atomic::AtomicU32,
}

/// An `IStream` over `bytes` that counts the calls it gets and misbehaves as told.
#[windows::core::implement(windows::Win32::System::Com::IStream)]
struct TestStream {
    bytes: Vec<u8>,
    at: std::sync::atomic::AtomicU64,
    behaviour: Behaviour,
    calls: std::sync::Arc<Calls>,
}

impl windows::Win32::System::Com::ISequentialStream_Impl for TestStream_Impl {
    fn Read(&self, pv: *mut c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        use std::sync::atomic::Ordering::SeqCst;
        let call = self.calls.reads.fetch_add(1, SeqCst);
        if self.behaviour.fail_read_from.is_some_and(|from| call >= from) {
            return E_FAIL;
        }
        let at = self.at.load(SeqCst) as usize;
        let rest = self.bytes.get(at..).unwrap_or_default();
        let n = rest.len().min(cb as usize);
        unsafe { std::ptr::copy_nonoverlapping(rest.as_ptr(), pv.cast::<u8>(), n) };
        self.at.store((at + n) as u64, SeqCst);
        if !pcbread.is_null() {
            unsafe { pcbread.write(if self.behaviour.overclaim { cb.saturating_add(4096) } else { n as u32 }) };
        }
        if n < cb as usize { S_FALSE } else { S_OK }
    }

    fn Write(&self, _pv: *const c_void, _cb: u32, _pcbwritten: *mut u32) -> HRESULT {
        E_NOTIMPL
    }
}

impl windows::Win32::System::Com::IStream_Impl for TestStream_Impl {
    fn Seek(&self, dlibmove: i64, dworigin: STREAM_SEEK, plibnewposition: *mut u64) -> windows::core::Result<()> {
        use std::sync::atomic::Ordering::SeqCst;
        let call = self.calls.seeks.fetch_add(1, SeqCst);
        if self.behaviour.fail_seek_from.is_some_and(|from| call >= from) {
            return Err(E_FAIL.into());
        }
        let length = self.behaviour.length.unwrap_or(self.bytes.len() as u64);
        let base = if dworigin == STREAM_SEEK_SET {
            0
        } else if dworigin == STREAM_SEEK_CUR {
            self.at.load(SeqCst)
        } else if dworigin == STREAM_SEEK_END {
            length
        } else {
            return Err(E_INVALIDARG.into());
        };
        let to = base.checked_add_signed(dlibmove).ok_or_else(|| windows::core::Error::from(E_INVALIDARG))?;
        self.at.store(to, SeqCst);
        if !plibnewposition.is_null() {
            unsafe { plibnewposition.write(to) };
        }
        Ok(())
    }
    fn SetSize(&self, _: u64) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn CopyTo(&self, _: windows::core::Ref<IStream>, _: u64, _: *mut u64, _: *mut u64) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn Commit(&self, _: &STGC) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn Revert(&self) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn LockRegion(&self, _: u64, _: u64, _: &LOCKTYPE) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn UnlockRegion(&self, _: u64, _: u64, _: u32) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn Stat(&self, _: *mut STATSTG, _: &STATFLAG) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn Clone(&self) -> windows::core::Result<IStream> {
        Err(E_NOTIMPL.into())
    }
}

/// A handler initialised with a [`TestStream`] over `bytes`, and the stream's call counts.
fn handler_on(factory: &IClassFactory, bytes: &[u8], behaviour: Behaviour) -> (IThumbnailProvider, std::sync::Arc<Calls>) {
    let calls = std::sync::Arc::new(Calls::default());
    let stream: IStream = TestStream { bytes: bytes.to_vec(), at: Default::default(), behaviour, calls: calls.clone() }.into();
    unsafe {
        let init: IInitializeWithStream = factory.CreateInstance(None).unwrap();
        init.Initialize(&stream, STGM_READ.0).unwrap();
        (init.cast().unwrap(), calls)
    }
}

/// `GetThumbnail(256)` on `provider`: the result, and the bitmap's size and bytes when there is one.
fn ask(provider: &IThumbnailProvider) -> (windows::core::Result<()>, Option<(i32, i32, Vec<u8>)>) {
    let mut hbmp = HBITMAP(std::ptr::dangling_mut::<c_void>());
    let mut alpha = WTS_ALPHATYPE(7);
    let result = unsafe { provider.GetThumbnail(256, &mut hbmp, &mut alpha) };
    if result.is_ok() {
        let (w, h, _, bytes) = pixels(hbmp);
        (result, Some((w, h, bytes)))
    } else {
        assert!(hbmp.0.is_null() && alpha == WTS_ALPHATYPE(0), "a failure returns no bitmap");
        (result, None)
    }
}

#[test]
fn initialize_reads_nothing_and_a_thumbnail_can_be_asked_for_twice() {
    use std::sync::atomic::Ordering::SeqCst;
    let _serial = serial();
    let factory = factory();
    let (provider, calls) = handler_on(&factory, &support::design(V2, 512, 384), Behaviour::default());
    assert_eq!((calls.reads.load(SeqCst), calls.seeks.load(SeqCst)), (0, 0), "Initialize reads nothing");
    let (first, a) = ask(&provider);
    let (second, b) = ask(&provider);
    assert_eq!((first, second), (Ok(()), Ok(())));
    assert_eq!(a.as_ref().map(|(w, h, _)| (*w, *h)), Some((256, 192)));
    assert_eq!(a, b, "the same bitmap both times");
}

#[test]
fn a_stream_that_fails_part_way_is_a_miss() {
    use std::sync::atomic::Ordering::SeqCst;
    let _serial = serial();
    let factory = factory();
    let design = support::design(V2, 512, 384);
    let (provider, calls) = handler_on(&factory, &design, Behaviour::default());
    assert_eq!(ask(&provider).0, Ok(()));
    let (reads, seeks) = (calls.reads.load(SeqCst), calls.seeks.load(SeqCst));
    assert!(reads > 0 && seeks > 0);
    for from in 0..reads {
        let (provider, _) = handler_on(&factory, &design, Behaviour { fail_read_from: Some(from), ..Behaviour::default() });
        assert_eq!(ask(&provider).0.map_err(|e| e.code()), Err(E_FAIL), "reads fail from call {from} of {reads}");
    }
    for from in 0..seeks {
        let (provider, _) = handler_on(&factory, &design, Behaviour { fail_seek_from: Some(from), ..Behaviour::default() });
        assert_eq!(ask(&provider).0.map_err(|e| e.code()), Err(E_FAIL), "seeks fail from call {from} of {seeks}");
    }
}

#[test]
fn a_stream_that_claims_more_than_it_read_is_held_to_the_buffer() {
    let _serial = serial();
    let factory = factory();
    for bytes in [support::design(V2, 512, 384), support::design(V1, 240, 150), b"not a zip".to_vec()] {
        let (provider, _) = handler_on(&factory, &bytes, Behaviour { overclaim: true, ..Behaviour::default() });
        let code = ask(&provider).0.map_err(|e| e.code());
        assert!(code == Ok(()) || code == Err(E_FAIL), "a thumbnail or a miss, never a caught panic: {code:?}");
    }
}

#[test]
fn a_stream_longer_than_the_file_bound_is_refused_unread() {
    use std::sync::atomic::Ordering::SeqCst;
    let _serial = serial();
    let factory = factory();
    let (provider, calls) = handler_on(&factory, &support::design(V2, 512, 384), Behaviour { length: Some((2 << 30) + 1), ..Behaviour::default() });
    assert_eq!(ask(&provider).0.map_err(|e| e.code()), Err(E_FAIL));
    assert_eq!(calls.reads.load(SeqCst), 0, "nothing read");
}

#[test]
fn the_factory_and_the_handler_answer_only_their_own_interfaces() {
    let _serial = serial();
    let clsid = GUID::from_u128(solidcraft_explorer::CLSID_U128);
    let get: GetClassObject = unsafe { std::mem::transmute(export(c"DllGetClassObject")) };
    let mut out = std::ptr::dangling_mut::<c_void>();
    assert_eq!(unsafe { get(&clsid, &IThumbnailProvider::IID, &mut out) }, E_NOINTERFACE);
    assert!(out.is_null());
    let mut out = std::ptr::null_mut();
    assert_eq!(unsafe { get(&clsid, &IUnknown::IID, &mut out) }, S_OK);
    let unknown = unsafe { IUnknown::from_raw(out) };
    let factory: IClassFactory = unknown.cast().unwrap();
    drop(unknown);
    let made: windows::core::Result<IClassFactory> = unsafe { factory.CreateInstance(None) };
    assert_eq!(made.map(|_| ()).map_err(|e| e.code()), Err(E_NOINTERFACE));
    let provider: IThumbnailProvider = unsafe { factory.CreateInstance(None) }.unwrap();
    assert!(provider.cast::<IInitializeWithStream>().is_ok());
    assert_eq!(provider.cast::<IClassFactory>().map(|_| ()).map_err(|e| e.code()), Err(E_NOINTERFACE));
    drop((provider, factory));
    assert_eq!(can_unload(), S_OK, "nothing is left alive");
}
